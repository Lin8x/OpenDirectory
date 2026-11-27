//! Remote scripts and actions queued from the Admin Center.

use std::time::Duration;

use od_core::protocol::{AgentJob, JobResult};
use od_core::{JobKind, Shell};

use crate::state::data_dir;
use crate::util;

const SCRIPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub async fn run(job: &AgentJob) -> JobResult {
    match &job.kind {
        JobKind::Script { shell, script, env } => match run_script(&job.id.to_string(), shell, script, env).await {
            Ok(r) => r,
            Err(e) => JobResult { exit_code: None, output: format!("Couldn't run the script: {e:#}"), success: false },
        },
        JobKind::RefreshInventory => {
            JobResult { exit_code: Some(0), output: "Inventory will be sent with the next check-in".into(), success: true }
        }
        JobKind::RestartAgent => JobResult { exit_code: Some(0), output: "Agent restarting".into(), success: true },
        JobKind::NotifyUser { title, message } => match crate::desktop::console_user().await {
            None => JobResult { exit_code: None, output: "Nobody is signed in at this device's screen".into(), success: false },
            Some(u) => {
                if crate::desktop::notify(title, message).await {
                    JobResult { exit_code: Some(0), output: format!("Shown to {}", u.name), success: true }
                } else {
                    JobResult {
                        exit_code: None,
                        output: format!("{} is signed in, but the message couldn't be shown", u.name),
                        success: false,
                    }
                }
            }
        },
        JobKind::Manage { action } => crate::manage::act(action, "").await,
        JobKind::Unknown => {
            JobResult { exit_code: None, output: "This job type needs a newer Endpoint Agent; update the agent".into(), success: false }
        }
        JobKind::ScanUpdates => {
            let r = crate::patches::scan().await;
            crate::agent::set_patch_report(r.clone());
            if r.error.is_empty() {
                let sec = r.pending.iter().filter(|p| p.security).count();
                let list: Vec<String> = r
                    .pending
                    .iter()
                    .take(50)
                    .map(|p| format!("{}{} {}", if p.security { "[security] " } else { "" }, p.title, p.id))
                    .collect();
                JobResult {
                    exit_code: Some(0),
                    output: format!(
                        "{} update(s) pending, {sec} security{}\n{}",
                        r.pending.len(),
                        if r.reboot_required { "; a restart is needed" } else { "" },
                        list.join("\n")
                    ),
                    success: true,
                }
            } else {
                JobResult { exit_code: None, output: r.error, success: false }
            }
        }
        JobKind::InstallUpdates { security_only, reboot } => {
            let res = crate::patches::install(*security_only).await;
            let r = crate::patches::scan().await;
            let needs_reboot = r.reboot_required;
            crate::agent::set_patch_report(r);
            match res {
                Ok((ok, out)) => {
                    let mut out = out;
                    if *reboot && needs_reboot {
                        crate::patches::schedule_reboot().await;
                        out.push_str("\nRestarting in 1 minute to finish installing.");
                    } else if needs_reboot {
                        out.push_str("\nA restart is needed to finish installing.");
                    }
                    JobResult { exit_code: Some(if ok { 0 } else { 1 }), output: out, success: ok }
                }
                Err(e) => JobResult { exit_code: None, output: format!("{e:#}"), success: false },
            }
        }
    }
}

fn plan(shell: &Shell) -> (&'static str, &'static str, Vec<&'static str>) {
    // (extension, program, args before the script path)
    match shell {
        Shell::Powershell => (
            "ps1",
            if cfg!(windows) { "powershell.exe" } else { "pwsh" },
            vec!["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"],
        ),
        Shell::Cmd => ("cmd", "cmd.exe", vec!["/D", "/C"]),
        Shell::Bash => ("sh", "bash", vec![]),
        Shell::Zsh => ("sh", "zsh", vec![]),
        Shell::Default => {
            if cfg!(windows) {
                ("cmd", "cmd.exe", vec!["/D", "/C"])
            } else {
                ("sh", "/bin/sh", vec![])
            }
        }
    }
}

pub async fn run_script(
    id: &str,
    shell: &Shell,
    script: &str,
    env: &std::collections::BTreeMap<String, String>,
) -> anyhow::Result<JobResult> {
    let (ext, program, mut args) = plan(shell);
    let dir = data_dir().join("jobs");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{id}.{ext}"));
    let body = if ext == "cmd" { script.replace('\n', "\r\n") } else { script.to_string() };
    std::fs::write(&path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    }
    let p = path.to_string_lossy().to_string();
    args.push(&p);
    let res = util::run_env(program, &args, env, SCRIPT_TIMEOUT).await;
    let _ = std::fs::remove_file(&path);
    let out = match res {
        Ok(o) => o,
        Err(e) => return Ok(JobResult { exit_code: None, output: format!("{e:#}"), success: false }),
    };
    let mut output = out.stdout;
    if !out.stderr.trim().is_empty() {
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&out.stderr);
    }
    Ok(JobResult { exit_code: out.code, success: out.code == Some(0), output })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn runs_shell_script_and_captures_output() {
        let dir = std::env::temp_dir().join(format!("od-agent-test-{}", Uuid::new_v4()));
        std::env::set_var("OD_AGENT_DIR", &dir);
        let job = AgentJob {
            id: Uuid::new_v4(),
            kind: JobKind::Script {
                shell: Shell::Default,
                script: "echo hello $OD_PARAM_X\necho oops >&2\nexit 3".into(),
                env: [("OD_PARAM_X".to_string(), "world".to_string())].into(),
            },
        };
        let r = run(&job).await;
        assert_eq!(r.exit_code, Some(3));
        assert!(!r.success);
        assert!(r.output.contains("hello world"));
        assert!(r.output.contains("oops"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
