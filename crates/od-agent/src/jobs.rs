//! Remote scripts and actions queued from the Admin Center.

use std::time::Duration;

use od_core::protocol::{AgentJob, JobResult};
use od_core::{JobKind, Shell};

use crate::state::data_dir;
use crate::util;

const SCRIPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub async fn run(job: &AgentJob) -> JobResult {
    match &job.kind {
        JobKind::Script { shell, script } => match run_script(&job.id.to_string(), shell, script).await {
            Ok(r) => r,
            Err(e) => JobResult { exit_code: None, output: format!("Couldn't run the script: {e:#}"), success: false },
        },
        JobKind::RefreshInventory => {
            JobResult { exit_code: Some(0), output: "Inventory will be sent with the next check-in".into(), success: true }
        }
        JobKind::RestartAgent => JobResult { exit_code: Some(0), output: "Agent restarting".into(), success: true },
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

async fn run_script(id: &str, shell: &Shell, script: &str) -> anyhow::Result<JobResult> {
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
    let res = util::run_with(program, &args, None, SCRIPT_TIMEOUT).await;
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
            kind: JobKind::Script { shell: Shell::Default, script: "echo hello\necho oops >&2\nexit 3".into() },
        };
        let r = run(&job).await;
        assert_eq!(r.exit_code, Some(3));
        assert!(!r.success);
        assert!(r.output.contains("hello"));
        assert!(r.output.contains("oops"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
