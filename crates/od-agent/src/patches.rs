//! Patch management: scan for pending OS updates and install them, using
//! Windows Update, apt, dnf or softwareupdate.

use std::time::Duration;

use chrono::Utc;
use od_core::{Patch, PatchReport};

#[allow(unused_imports)]
use crate::util::{run, run_with, which};

const LONG: Duration = Duration::from_secs(3 * 60 * 60);

fn report(source: &str, pending: Vec<Patch>, reboot_required: bool) -> PatchReport {
    PatchReport { scanned_at: Some(Utc::now()), pending, reboot_required, source: source.into(), error: String::new() }
}
#[allow(dead_code)]
fn failed(source: &str, error: impl Into<String>) -> PatchReport {
    PatchReport { scanned_at: Some(Utc::now()), source: source.into(), error: error.into(), ..Default::default() }
}

/// Parse `apt list --upgradable` output.
pub fn parse_apt(out: &str) -> Vec<Patch> {
    out.lines()
        .filter(|l| l.contains("[upgradable from"))
        .filter_map(|l| {
            let (name, rest) = l.split_once('/')?;
            let mut parts = rest.split_whitespace();
            let suite = parts.next().unwrap_or("");
            let version = parts.next().unwrap_or("");
            Some(Patch { title: name.to_string(), id: version.to_string(), security: suite.contains("-security"), severity: String::new() })
        })
        .collect()
}

/// Parse `dnf check-update -q` output (name.arch version repo).
pub fn parse_dnf(out: &str, security: &[String]) -> Vec<Patch> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() != 3 || l.starts_with(' ') || f[0].starts_with("Obsoleting") {
                return None;
            }
            let name = f[0].rsplit_once('.').map(|(n, _)| n).unwrap_or(f[0]);
            Some(Patch {
                title: name.to_string(),
                id: f[1].to_string(),
                security: security.iter().any(|s| s.starts_with(name)),
                severity: String::new(),
            })
        })
        .collect()
}

/// Parse `softwareupdate -l` output.
pub fn parse_softwareupdate(out: &str) -> (Vec<Patch>, bool) {
    let mut v = vec![];
    let mut reboot = false;
    let mut label: Option<String> = None;
    for l in out.lines() {
        let t = l.trim();
        if let Some(x) = t.strip_prefix("* Label: ").or_else(|| t.strip_prefix("* ")) {
            label = Some(x.trim().to_string());
        } else if let (Some(lb), true) = (&label, t.starts_with("Title:")) {
            let title = t.trim_start_matches("Title:").split(", Version:").next().unwrap_or("").trim().to_string();
            let needs = t.contains("Action: restart") || t.contains("[restart]");
            reboot |= needs;
            let security = title.to_lowercase().contains("security") || lb.to_lowercase().contains("security");
            v.push(Patch { title, id: lb.clone(), security, severity: String::new() });
            label = None;
        }
    }
    (v, reboot)
}

// ================================================================== Linux
#[cfg(target_os = "linux")]
pub async fn scan() -> PatchReport {
    let reboot = std::path::Path::new("/var/run/reboot-required").exists();
    if which("apt-get") {
        let _ = run_with("apt-get", &["update", "-qq"], None, Duration::from_secs(600)).await;
        return match run("apt", &["list", "--upgradable"]).await {
            Ok(o) => report("apt", parse_apt(&o.stdout), reboot),
            Err(e) => failed("apt", format!("{e:#}")),
        };
    }
    if which("dnf") {
        let sec = run_with("dnf", &["updateinfo", "list", "--security", "-q"], None, Duration::from_secs(600))
            .await
            .map(|o| o.stdout.lines().filter_map(|l| l.split_whitespace().nth(2).map(str::to_string)).collect::<Vec<_>>())
            .unwrap_or_default();
        let reboot = reboot || run("needs-restarting", &["-r"]).await.map(|o| o.code == Some(1)).unwrap_or(false);
        return match run_with("dnf", &["check-update", "-q"], None, Duration::from_secs(600)).await {
            Ok(o) if o.code == Some(0) || o.code == Some(100) => report("dnf", parse_dnf(&o.stdout, &sec), reboot),
            Ok(o) => failed("dnf", o.message()),
            Err(e) => failed("dnf", format!("{e:#}")),
        };
    }
    failed("", "Only apt and dnf based distributions are supported for patch scans")
}

#[cfg(target_os = "linux")]
pub async fn install(security_only: bool) -> anyhow::Result<(bool, String)> {
    let o = if which("apt-get") {
        if security_only && which("unattended-upgrade") {
            run_with("unattended-upgrade", &["-v"], None, LONG).await?
        } else {
            let _ = run_with("apt-get", &["update", "-qq"], None, Duration::from_secs(600)).await;
            let mut o = crate::util::run_env(
                "apt-get",
                &["-y", "-o", "Dpkg::Options::=--force-confold", "upgrade"],
                &[("DEBIAN_FRONTEND".to_string(), "noninteractive".to_string())].into(),
                LONG,
            )
            .await?;
            o.stdout = o.stdout.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
            o
        }
    } else if which("dnf") {
        let args: &[&str] = if security_only { &["-y", "upgrade", "--security"] } else { &["-y", "upgrade"] };
        run_with("dnf", args, None, LONG).await?
    } else {
        anyhow::bail!("Only apt and dnf based distributions are supported");
    };
    Ok((o.ok(), format!("{}\n{}", o.stdout.trim(), o.stderr.trim()).trim().to_string()))
}

// ================================================================== macOS
#[cfg(target_os = "macos")]
pub async fn scan() -> PatchReport {
    match run_with("softwareupdate", &["-l"], None, Duration::from_secs(900)).await {
        Ok(o) => {
            let (pending, reboot) = parse_softwareupdate(&format!("{}\n{}", o.stdout, o.stderr));
            report("softwareupdate", pending, reboot)
        }
        Err(e) => failed("softwareupdate", format!("{e:#}")),
    }
}

#[cfg(target_os = "macos")]
pub async fn install(security_only: bool) -> anyhow::Result<(bool, String)> {
    let o = if security_only {
        let (pending, _) = parse_softwareupdate(&run_with("softwareupdate", &["-l"], None, Duration::from_secs(900)).await?.stdout);
        let labels: Vec<String> = pending.into_iter().filter(|p| p.security).map(|p| p.id).collect();
        if labels.is_empty() {
            return Ok((true, "No security updates pending".into()));
        }
        let mut args = vec!["-i"];
        args.extend(labels.iter().map(String::as_str));
        run_with("softwareupdate", &args, None, LONG).await?
    } else {
        run_with("softwareupdate", &["-i", "-a", "--agree-to-license"], None, LONG).await?
    };
    Ok((o.ok(), format!("{}\n{}", o.stdout.trim(), o.stderr.trim()).trim().to_string()))
}

// ================================================================== Windows
#[cfg(target_os = "windows")]
const WU_SCAN: &str = r#"
$s = New-Object -ComObject Microsoft.Update.Session
$r = $s.CreateUpdateSearcher().Search("IsInstalled=0 and IsHidden=0 and Type='Software'")
$list = @($r.Updates | ForEach-Object {
  $cats = @($_.Categories | ForEach-Object { $_.Name })
  [pscustomobject]@{ title = $_.Title; id = (@($_.KBArticleIDs) | ForEach-Object { "KB$_" }) -join ','; security = ($cats -contains 'Security Updates' -or $cats -contains 'Critical Updates'); severity = [string]$_.MsrcSeverity }
})
$reboot = (New-Object -ComObject Microsoft.Update.SystemInfo).RebootRequired
[pscustomobject]@{ pending = $list; reboot = $reboot } | ConvertTo-Json -Depth 4 -Compress
"#;

#[cfg(target_os = "windows")]
pub async fn scan() -> PatchReport {
    let o = match run_with(
        "powershell.exe",
        &["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", WU_SCAN],
        None,
        Duration::from_secs(1800),
    )
    .await
    {
        Ok(o) if o.ok() => o,
        Ok(o) => return failed("Windows Update", o.message()),
        Err(e) => return failed("Windows Update", format!("{e:#}")),
    };
    #[derive(serde::Deserialize)]
    struct R {
        #[serde(default, deserialize_with = "one_or_many")]
        pending: Vec<Patch>,
        #[serde(default)]
        reboot: bool,
    }
    fn one_or_many<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Patch>, D::Error> {
        use serde::Deserialize;
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum X {
            One(Patch),
            Many(Vec<Patch>),
            Null(()),
        }
        Ok(match X::deserialize(d)? {
            X::One(p) => vec![p],
            X::Many(v) => v,
            X::Null(()) => vec![],
        })
    }
    match serde_json::from_str::<R>(o.stdout.trim()) {
        Ok(r) => report("Windows Update", r.pending, r.reboot),
        Err(e) => failed("Windows Update", format!("Couldn't read the scan result: {e}")),
    }
}

#[cfg(target_os = "windows")]
pub async fn install(security_only: bool) -> anyhow::Result<(bool, String)> {
    let filter = if security_only {
        "| Where-Object { $c = @($_.Categories | ForEach-Object { $_.Name }); $c -contains 'Security Updates' -or $c -contains 'Critical Updates' }"
    } else {
        ""
    };
    let script = format!(
        r#"$s = New-Object -ComObject Microsoft.Update.Session
$r = $s.CreateUpdateSearcher().Search("IsInstalled=0 and IsHidden=0 and Type='Software'")
$c = New-Object -ComObject Microsoft.Update.UpdateColl
$r.Updates {filter} | ForEach-Object {{ if (-not $_.EulaAccepted) {{ $_.AcceptEula() }}; [void]$c.Add($_) }}
if ($c.Count -eq 0) {{ 'No updates to install'; exit 0 }}
$d = $s.CreateUpdateDownloader(); $d.Updates = $c; [void]$d.Download()
$i = $s.CreateUpdateInstaller(); $i.Updates = $c; $res = $i.Install()
for ($n = 0; $n -lt $c.Count; $n++) {{ "{{0}}: {{1}}" -f $c.Item($n).Title, $res.GetUpdateResult($n).ResultCode }}
"Result code: $($res.ResultCode). Restart needed: $($res.RebootRequired)"
if ($res.ResultCode -ne 2 -and $res.ResultCode -ne 3) {{ exit 1 }}"#
    );
    let o = run_with(
        "powershell.exe",
        &["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script],
        None,
        LONG,
    )
    .await?;
    Ok((o.ok(), format!("{}\n{}", o.stdout.trim(), o.stderr.trim()).trim().to_string()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub async fn scan() -> PatchReport {
    failed("", "Unsupported OS")
}
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub async fn install(_: bool) -> anyhow::Result<(bool, String)> {
    anyhow::bail!("Unsupported OS")
}

/// Restart the computer in a minute, with a notice to signed-in users.
pub async fn schedule_reboot() {
    #[cfg(windows)]
    let _ =
        run("shutdown.exe", &["/r", "/t", "60", "/c", "Updates were installed by your IT team. This computer restarts in 1 minute."]).await;
    #[cfg(not(windows))]
    let _ = run("shutdown", &["-r", "+1", "Updates were installed by your IT team. Restarting in 1 minute."]).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apt() {
        let out = "Listing...\nopenssl/jammy-security 3.0.2-0ubuntu1.15 amd64 [upgradable from: 3.0.2-0ubuntu1.14]\nvim/jammy-updates 2:8.2 amd64 [upgradable from: 2:8.1]\n";
        let p = parse_apt(out);
        assert_eq!(p.len(), 2);
        assert!(p[0].security && !p[1].security);
        assert_eq!(p[0].title, "openssl");
    }

    #[test]
    fn parses_dnf() {
        let out = "\nopenssl.x86_64   1:3.0.7-25.el9   baseos\nkernel.x86_64   5.14.0-427.el9   baseos\n";
        let p = parse_dnf(out, &["openssl-1:3.0.7-25.el9.x86_64".into()]);
        assert_eq!(p.len(), 2);
        assert!(p[0].security && !p[1].security);
    }

    #[test]
    fn parses_softwareupdate() {
        let out = "Software Update found the following new or updated software:\n* Label: macOS Sonoma 14.5-23F79\n\tTitle: macOS Sonoma 14.5, Version: 14.5, Size: 1G, Recommended: YES, Action: restart,\n* Label: Safari17.5\n\tTitle: Safari, Version: 17.5, Size: 100K, Recommended: YES,\n";
        let (p, reboot) = parse_softwareupdate(out);
        assert_eq!(p.len(), 2);
        assert!(reboot);
        assert_eq!(p[0].id, "macOS Sonoma 14.5-23F79");
    }
}
