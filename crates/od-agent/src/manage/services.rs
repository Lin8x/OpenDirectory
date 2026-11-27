//! Services: the Windows Service Control Manager, systemd units on Linux and
//! launchd jobs on macOS.

#[allow(unused_imports)]
use anyhow::{bail, Context, Result};
use od_core::manage::{ServiceControl, ServiceInfo, ServiceReport, StartupType};

#[allow(unused_imports)]
use super::must;

// ====================================================================== Linux
#[cfg(target_os = "linux")]
pub async fn list() -> Result<ServiceReport> {
    use std::collections::BTreeMap;
    if !crate::util::which("systemctl") {
        bail!("This endpoint doesn't use systemd, so its services can't be managed yet");
    }
    let units = must("systemctl", &["list-units", "--type=service", "--all", "--no-legend", "--no-pager", "--plain"], 60).await?;
    let files = must("systemctl", &["list-unit-files", "--type=service", "--no-legend", "--no-pager"], 60).await?;
    let mut map: BTreeMap<String, ServiceInfo> = BTreeMap::new();
    for (name, startup) in parse_unit_files(&files) {
        map.insert(
            name.clone(),
            ServiceInfo {
                configurable: !matches!(startup.as_str(), "static" | "generated" | "transient" | "alias"),
                name,
                state: "stopped".into(),
                startup,
                ..Default::default()
            },
        );
    }
    for u in parse_units(&units) {
        let e = map.entry(u.name.clone()).or_insert_with(|| ServiceInfo {
            name: u.name.clone(),
            startup: "static".into(),
            configurable: false,
            ..Default::default()
        });
        e.state = u.state;
        e.description = u.description.clone();
        e.display_name = u.description;
    }
    // Main PID and account for everything that's running.
    let running: Vec<String> = map.values().filter(|s| s.state == "running").map(|s| s.name.clone()).collect();
    for chunk in running.chunks(200) {
        let mut args = vec!["show", "-p", "Id", "-p", "MainPID", "-p", "User", "--"];
        args.extend(chunk.iter().map(String::as_str));
        if let Ok(out) = must("systemctl", &args, 60).await {
            for block in out.split("\n\n") {
                let kv: BTreeMap<&str, &str> = block.lines().filter_map(|l| l.split_once('=')).collect();
                if let Some(s) = kv.get("Id").and_then(|id| map.get_mut(*id)) {
                    s.pid = kv.get("MainPID").and_then(|p| p.parse().ok()).filter(|p| *p > 0);
                    s.account = kv.get("User").filter(|u| !u.is_empty()).map(|u| u.to_string()).unwrap_or_else(|| "root".into());
                }
            }
        }
    }
    Ok(ServiceReport { manager: "systemd".into(), services: map.into_values().collect() })
}

#[cfg(any(target_os = "linux", test))]
struct Unit {
    name: String,
    state: String,
    description: String,
}

/// `systemctl list-units --plain --no-legend`: UNIT LOAD ACTIVE SUB DESCRIPTION…
#[cfg(any(target_os = "linux", test))]
fn parse_units(out: &str) -> Vec<Unit> {
    out.lines()
        .filter_map(|l| {
            let l = l.trim_start_matches(['●', '*', ' ']);
            let mut it = l.split_whitespace();
            let name = it.next()?.to_string();
            let load = it.next()?;
            let active = it.next()?;
            let sub = it.next()?;
            let description = it.collect::<Vec<_>>().join(" ");
            if load == "not-found" {
                return None;
            }
            let state = match (active, sub) {
                ("active", "running") => "running",
                ("active", "exited") => "exited",
                ("active", _) => "running",
                ("failed", _) => "failed",
                ("activating", _) | ("reloading", _) => "starting",
                ("deactivating", _) => "stopping",
                _ => "stopped",
            };
            Some(Unit { name, state: state.into(), description })
        })
        .collect()
}

/// `systemctl list-unit-files`: UNIT STATE [PRESET], mapped to SCM startup types.
#[cfg(any(target_os = "linux", test))]
fn parse_unit_files(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?;
            let state = it.next()?;
            if name.contains("@.") {
                return None; // templates aren't services you can start as is
            }
            let startup = match state {
                "enabled" | "enabled-runtime" => "automatic",
                "disabled" => "manual",
                "masked" | "masked-runtime" => "disabled",
                other => other,
            };
            Some((name.to_string(), startup.to_string()))
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub async fn control(name: &str, c: ServiceControl) -> Result<String> {
    let verb = match c {
        ServiceControl::Start => "start",
        ServiceControl::Stop => "stop",
        ServiceControl::Restart => "restart",
    };
    must("systemctl", &[verb, "--no-ask-password", "--", name], 180).await?;
    let state = must("systemctl", &["is-active", "--", name], 30).await.unwrap_or_else(|e| e.to_string());
    Ok(format!("systemctl {verb} {name}: now {}", state.lines().next().unwrap_or("").trim()))
}

#[cfg(target_os = "linux")]
pub async fn set_startup(name: &str, s: StartupType) -> Result<String> {
    match s {
        StartupType::Automatic => {
            must("systemctl", &["unmask", "--no-ask-password", "--", name], 60).await?;
            must("systemctl", &["enable", "--no-ask-password", "--", name], 60).await
        }
        StartupType::Manual => {
            must("systemctl", &["unmask", "--no-ask-password", "--", name], 60).await?;
            must("systemctl", &["disable", "--no-ask-password", "--", name], 60).await
        }
        StartupType::Disabled => must("systemctl", &["mask", "--no-ask-password", "--", name], 60).await,
        StartupType::AutomaticDelayed => bail!("Automatic (Delayed Start) is a Windows startup type; use Automatic on systemd"),
    }
    .map(|o| if o.is_empty() { format!("{name}: startup type set to {}", od_core::manage::startup_label(s)) } else { o })
}

// ==================================================================== Windows
#[cfg(target_os = "windows")]
pub async fn list() -> Result<ServiceReport> {
    #[derive(serde::Deserialize)]
    struct Row {
        name: String,
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        state: Option<String>,
        #[serde(default)]
        start: Option<String>,
        #[serde(default)]
        delayed: Option<bool>,
        #[serde(default)]
        pid: Option<u32>,
        #[serde(default)]
        account: Option<String>,
    }
    let ps = r#"@(Get-CimInstance Win32_Service | ForEach-Object { [pscustomobject]@{ name=$_.Name; display_name=$_.DisplayName; description=$_.Description; state=$_.State; start=$_.StartMode; delayed=$_.DelayedAutoStart; pid=$_.ProcessId; account=$_.StartName } }) | ConvertTo-Json -Compress -Depth 2"#;
    let o = crate::util::powershell(ps, None).await?;
    if !o.ok() {
        bail!("Get-CimInstance Win32_Service failed: {}", o.message());
    }
    let rows: Vec<Row> = serde_json::from_str(o.stdout.trim()).context("reading the service list")?;
    let services = rows
        .into_iter()
        .map(|r| {
            let state = r.state.unwrap_or_default().to_lowercase();
            let state = match state.as_str() {
                "start pending" => "starting".into(),
                "stop pending" => "stopping".into(),
                "continue pending" | "pause pending" => "pending".into(),
                _ => state,
            };
            let startup = match r.start.unwrap_or_default().as_str() {
                "Auto" if r.delayed == Some(true) => "automatic_delayed",
                "Auto" => "automatic",
                "Manual" => "manual",
                "Disabled" => "disabled",
                "Boot" => "boot",
                "System" => "system",
                _ => "unknown",
            };
            ServiceInfo {
                configurable: !matches!(startup, "boot" | "system"),
                name: r.name,
                display_name: r.display_name.unwrap_or_default(),
                description: r.description.unwrap_or_default(),
                state,
                startup: startup.into(),
                pid: r.pid.filter(|p| *p > 0),
                account: r.account.unwrap_or_default(),
            }
        })
        .collect();
    Ok(ServiceReport { manager: "scm".into(), services })
}

#[cfg(target_os = "windows")]
pub async fn control(name: &str, c: ServiceControl) -> Result<String> {
    // Names are validated (no quotes), so single quotes keep them literal.
    let cmd = match c {
        ServiceControl::Start => "Start-Service",
        ServiceControl::Stop => "Stop-Service -Force",
        ServiceControl::Restart => "Restart-Service -Force",
    };
    let ps = format!("$ErrorActionPreference='Stop'; {cmd} -Name '{name}'; (Get-Service -Name '{name}').Status");
    let o = crate::util::powershell(&ps, None).await?;
    if !o.ok() {
        bail!("{}", super::combined(&o));
    }
    Ok(format!("{name} is now {}", o.stdout.trim()))
}

#[cfg(target_os = "windows")]
pub async fn set_startup(name: &str, s: StartupType) -> Result<String> {
    let start = match s {
        StartupType::Automatic => "auto",
        StartupType::AutomaticDelayed => "delayed-auto",
        StartupType::Manual => "demand",
        StartupType::Disabled => "disabled",
    };
    must("sc.exe", &["config", name, "start=", start], 60).await
}

// ====================================================================== macOS
#[cfg(target_os = "macos")]
pub async fn list() -> Result<ServiceReport> {
    let out = must("launchctl", &["list"], 60).await?;
    let disabled = must("launchctl", &["print-disabled", "system"], 60).await.unwrap_or_default();
    let off: std::collections::HashSet<String> = disabled
        .lines()
        .filter(|l| l.contains("=> disabled") || l.contains("=> true"))
        .filter_map(|l| l.split('"').nth(1).map(str::to_string))
        .collect();
    let services = out
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut it = l.split('\t');
            let pid = it.next()?.trim();
            let status = it.next()?.trim();
            let label = it.next()?.trim().to_string();
            let pid: Option<u32> = pid.parse().ok();
            let state = if pid.is_some() {
                "running"
            } else if status != "0" && status != "-" {
                "failed"
            } else {
                "stopped"
            };
            Some(ServiceInfo {
                startup: if off.contains(&label) { "disabled".into() } else { "automatic".into() },
                display_name: label.clone(),
                name: label,
                state: state.into(),
                pid,
                account: "root".into(),
                configurable: true,
                ..Default::default()
            })
        })
        .collect();
    Ok(ServiceReport { manager: "launchd".into(), services })
}

#[cfg(target_os = "macos")]
pub async fn control(name: &str, c: ServiceControl) -> Result<String> {
    let target = format!("system/{name}");
    match c {
        ServiceControl::Start => must("launchctl", &["kickstart", &target], 120).await,
        ServiceControl::Stop => must("launchctl", &["kill", "SIGTERM", &target], 120).await,
        ServiceControl::Restart => must("launchctl", &["kickstart", "-k", &target], 120).await,
    }
    .map(|o| if o.is_empty() { format!("launchctl: {name} done") } else { o })
}

#[cfg(target_os = "macos")]
pub async fn set_startup(name: &str, s: StartupType) -> Result<String> {
    let target = format!("system/{name}");
    match s {
        StartupType::Automatic => must("launchctl", &["enable", &target], 60).await,
        StartupType::Disabled => must("launchctl", &["disable", &target], 60).await,
        _ => bail!("launchd jobs are either Automatic (enabled) or Disabled"),
    }
    .map(|_| format!("{name}: startup type set to {}", od_core::manage::startup_label(s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_systemctl() {
        let units = "ssh.service loaded active running OpenBSD Secure Shell server\n\
                     ● foo.service not-found inactive dead foo.service\n\
                     cups.service loaded failed failed CUPS Scheduler\n";
        let u = parse_units(units);
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].name, "ssh.service");
        assert_eq!(u[0].state, "running");
        assert_eq!(u[0].description, "OpenBSD Secure Shell server");
        assert_eq!(u[1].state, "failed");
        let files = "ssh.service enabled enabled\ngetty@.service enabled enabled\nrescue.service static -\nbad.service masked enabled\n";
        let f = parse_unit_files(files);
        assert_eq!(
            f,
            vec![
                ("ssh.service".to_string(), "automatic".to_string()),
                ("rescue.service".to_string(), "static".to_string()),
                ("bad.service".to_string(), "disabled".to_string())
            ]
        );
    }
}
