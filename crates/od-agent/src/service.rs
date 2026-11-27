//! Installing the agent as an always-on background service:
//! a systemd unit on Linux, a launchd daemon on macOS and a Windows Service
//! on Windows. Each starts at boot, runs as root/SYSTEM before anyone signs
//! in, and is restarted automatically if it stops.
//!
//! The agent is headless: it has no tray icon, window or other UI of its
//! own. It isn't hidden either. It runs under its own service name, lives in
//! the standard program folder, and `opendirectory-agent status` shows it.
//! By default the service runs at low priority (see [`Priority`]) so it
//! yields CPU and disk to whatever the user is doing.

#[allow(unused_imports)]
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

#[cfg_attr(not(windows), allow(dead_code))]
pub const SERVICE_NAME: &str = "OpenDirectoryAgent";
pub const DISPLAY_NAME: &str = "OpenDirectory Endpoint Agent";

/// Scheduling priority for the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Priority {
    /// Below-normal CPU and I/O priority: the agent yields to the user's apps.
    #[default]
    Low,
    /// The OS default priority.
    Normal,
}

impl Priority {
    fn path() -> PathBuf {
        crate::state::data_dir().join("service-priority")
    }
    /// The priority chosen at install time (low if never set).
    pub fn load() -> Priority {
        match std::fs::read_to_string(Self::path()).map(|s| s.trim().to_string()).as_deref() {
            Ok("normal") => Priority::Normal,
            _ => Priority::Low,
        }
    }
    fn save(self) -> Result<()> {
        std::fs::create_dir_all(crate::state::data_dir())?;
        std::fs::write(Self::path(), self.label())?;
        Ok(())
    }
    pub fn label(self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Normal => "normal",
        }
    }
}

/// Lower this process's own priority. The service managers on Linux and
/// macOS already do this from the unit file; this covers Windows and
/// `opendirectory-agent run` started by hand.
pub fn apply_priority(p: Priority) {
    if p == Priority::Normal {
        return;
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS};
        SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS);
    }
    #[cfg(unix)]
    {
        // Nice 10 for this process; children (scripts, package managers) inherit it.
        let _ = std::process::Command::new("renice").args(["-n", "10", "-p", &std::process::id().to_string()]).output();
    }
}

pub fn install_path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into()))
            .join("OpenDirectory")
            .join("opendirectory-agent.exe")
    } else {
        PathBuf::from("/usr/local/bin/opendirectory-agent")
    }
}

/// Copy the running binary to the standard location (if it isn't there already).
fn copy_self() -> Result<PathBuf> {
    let me = std::env::current_exe()?.canonicalize()?;
    let dest = install_path();
    if dest.exists() && dest.canonicalize().ok() == Some(me.clone()) {
        return Ok(dest);
    }
    std::fs::create_dir_all(dest.parent().unwrap())?;
    // Copy to a temp name then rename, so a running copy can be replaced.
    let tmp = dest.with_extension("new");
    std::fs::copy(&me, &tmp).with_context(|| format!("copy agent to {}", dest.display()))?;
    #[cfg(windows)]
    {
        let _ = std::fs::rename(&dest, dest.with_extension("old"));
    }
    std::fs::rename(&tmp, &dest)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(dest)
}

#[cfg(unix)]
fn require_root() -> Result<()> {
    let out = std::process::Command::new("id").arg("-u").output()?;
    if String::from_utf8_lossy(&out.stdout).trim() != "0" {
        bail!("Run this as root (for example with sudo)");
    }
    Ok(())
}

fn sh(program: &str, args: &[&str]) -> Result<std::process::Output> {
    std::process::Command::new(program).args(args).output().with_context(|| format!("run {program}"))
}

// ---------------------------------------------------------------- Linux
#[cfg(target_os = "linux")]
const UNIT_PATH: &str = "/etc/systemd/system/opendirectory-agent.service";

#[cfg(target_os = "linux")]
fn unit_text(bin: &std::path::Path, p: Priority) -> String {
    // Low priority: nice 10, a small CPU and I/O share when the system is
    // busy (full speed when it's idle), and the lowest best-effort disk I/O priority.
    let tuning = match p {
        Priority::Low => "Nice=10\nCPUWeight=20\nIOWeight=20\nIOSchedulingClass=best-effort\nIOSchedulingPriority=7\n",
        Priority::Normal => "",
    };
    format!(
        "[Unit]\nDescription={DISPLAY_NAME}\nDocumentation=https://github.com/Lin8x/OpenDirectory\nAfter=network-online.target\nWants=network-online.target\n\n\
         [Service]\nType=simple\nExecStart={} run\nRestart=always\nRestartSec=5\n{tuning}\n[Install]\nWantedBy=multi-user.target\n",
        bin.display()
    )
}

/// Bring an existing unit file up to date after a self-update, without
/// restarting. The new settings apply at the next service start.
#[cfg(target_os = "linux")]
pub fn refresh_definition() {
    let Ok(cur) = std::fs::read_to_string(UNIT_PATH) else { return };
    let want = unit_text(&install_path(), Priority::load());
    if cur != want && std::fs::write(UNIT_PATH, want).is_ok() {
        let _ = sh("systemctl", &["daemon-reload"]);
        tracing::info!("Updated the service definition in {UNIT_PATH}");
    }
}

#[cfg(target_os = "linux")]
pub fn install(p: Priority) -> Result<()> {
    require_root()?;
    let bin = copy_self()?;
    p.save()?;
    std::fs::write(UNIT_PATH, unit_text(&bin, p))?;
    sh("systemctl", &["daemon-reload"])?;
    let o = sh("systemctl", &["enable", "--now", "opendirectory-agent.service"])?;
    if !o.status.success() {
        bail!("systemctl enable failed: {}", String::from_utf8_lossy(&o.stderr));
    }
    let _ = sh("systemctl", &["restart", "opendirectory-agent.service"]);
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn uninstall() -> Result<()> {
    require_root()?;
    let _ = sh("systemctl", &["disable", "--now", "opendirectory-agent.service"]);
    let _ = std::fs::remove_file(UNIT_PATH);
    let _ = sh("systemctl", &["daemon-reload"]);
    let _ = std::fs::remove_file(install_path());
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn status() -> String {
    sh("systemctl", &["is-active", "opendirectory-agent.service"])
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

// ---------------------------------------------------------------- macOS
#[cfg(target_os = "macos")]
const PLIST_PATH: &str = "/Library/LaunchDaemons/dev.opendirectory.agent.plist";

#[cfg(target_os = "macos")]
fn plist_text(bin: &std::path::Path, p: Priority) -> String {
    // Background: launchd throttles CPU and disk I/O when the Mac is busy.
    let tuning = match p {
        Priority::Low => "  <key>ProcessType</key><string>Background</string>\n  <key>LowPriorityIO</key><true/>\n  <key>Nice</key><integer>10</integer>\n",
        Priority::Normal => "",
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.opendirectory.agent</string>
  <key>ProgramArguments</key><array><string>{}</string><string>run</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>5</integer>
  <key>StandardOutPath</key><string>/Library/Logs/OpenDirectory/agent.log</string>
  <key>StandardErrorPath</key><string>/Library/Logs/OpenDirectory/agent.log</string>
{tuning}</dict>
</plist>
"#,
        bin.display()
    )
}

/// Bring an existing launchd definition up to date after a self-update. The
/// new settings apply the next time launchd loads it.
#[cfg(target_os = "macos")]
pub fn refresh_definition() {
    let Ok(cur) = std::fs::read_to_string(PLIST_PATH) else { return };
    let want = plist_text(&install_path(), Priority::load());
    if cur != want && std::fs::write(PLIST_PATH, want).is_ok() {
        tracing::info!("Updated the service definition in {PLIST_PATH}");
    }
}

#[cfg(target_os = "macos")]
pub fn install(p: Priority) -> Result<()> {
    require_root()?;
    let bin = copy_self()?;
    p.save()?;
    std::fs::create_dir_all("/Library/Logs/OpenDirectory")?;
    let plist = plist_text(&bin, p);
    let _ = sh("launchctl", &["bootout", "system", PLIST_PATH]);
    std::fs::write(PLIST_PATH, plist)?;
    let o = sh("launchctl", &["bootstrap", "system", PLIST_PATH])?;
    if !o.status.success() {
        bail!("launchctl bootstrap failed: {}", String::from_utf8_lossy(&o.stderr));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<()> {
    require_root()?;
    let _ = sh("launchctl", &["bootout", "system", PLIST_PATH]);
    let _ = std::fs::remove_file(PLIST_PATH);
    let _ = std::fs::remove_file(install_path());
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn status() -> String {
    match sh("launchctl", &["print", "system/dev.opendirectory.agent"]) {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            if s.contains("state = running") {
                "active".into()
            } else {
                "loaded".into()
            }
        }
        _ => "not installed".into(),
    }
}

// ---------------------------------------------------------------- Windows
#[cfg(windows)]
pub fn refresh_definition() {}

#[cfg(windows)]
pub fn install(p: Priority) -> Result<()> {
    use std::ffi::OsString;
    use windows_service::service::{ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let bin = copy_self()?;
    p.save()?;
    let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)
        .context("Open the Service Control Manager (run this from an elevated prompt)")?;
    if let Ok(existing) = mgr.open_service(SERVICE_NAME, ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS) {
        let _ = existing.stop();
        std::thread::sleep(std::time::Duration::from_secs(2));
        existing.delete()?;
        drop(existing);
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    let info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: bin,
        launch_arguments: vec![OsString::from("service")],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };
    let svc = mgr.create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)?;
    svc.set_description("Keeps this computer managed by OpenDirectory: inventory, Group Policy, sign-in accounts and remote scripts.")?;
    // Restart automatically if the agent ever exits unexpectedly.
    let _ = sh("sc.exe", &["failure", SERVICE_NAME, "reset=", "86400", "actions=", "restart/5000/restart/5000/restart/30000"]);
    svc.start::<&str>(&[])?;
    Ok(())
}

#[cfg(windows)]
pub fn uninstall() -> Result<()> {
    use windows_service::service::ServiceAccess;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    if let Ok(svc) = mgr.open_service(SERVICE_NAME, ServiceAccess::STOP | ServiceAccess::DELETE) {
        let _ = svc.stop();
        std::thread::sleep(std::time::Duration::from_secs(2));
        svc.delete()?;
    }
    Ok(())
}

#[cfg(windows)]
pub fn status() -> String {
    use windows_service::service::{ServiceAccess, ServiceState};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    let Ok(mgr) = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT) else { return "unknown".into() };
    match mgr.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS).and_then(|s| s.query_status()) {
        Ok(st) if st.current_state == ServiceState::Running => "active".into(),
        Ok(st) => format!("{:?}", st.current_state).to_lowercase(),
        Err(_) => "not installed".into(),
    }
}

/// Entry point when started by the Windows Service Control Manager.
#[cfg(windows)]
pub mod win {
    use std::ffi::OsString;
    use std::time::Duration;

    use anyhow::Result;
    use tokio::sync::watch;
    use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType};
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::{define_windows_service, service_dispatcher};

    use super::SERVICE_NAME;

    define_windows_service!(ffi_service_main, service_main);

    pub fn run_dispatcher() -> Result<()> {
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
        Ok(())
    }

    fn service_main(_args: Vec<OsString>) {
        if let Err(e) = run_service() {
            tracing::error!("service failed: {e:#}");
        }
    }

    fn run_service() -> Result<()> {
        let (tx, rx) = watch::channel(false);
        let handler = move |event| match event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = tx.send(true);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let status = service_control_handler::register(SERVICE_NAME, handler)?;
        let set = |state: ServiceState, accept: ServiceControlAccept| ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        };
        status.set_service_status(set(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN))?;
        crate::service::apply_priority(crate::service::Priority::load());
        let rt = crate::runtime()?;
        let restart = rt.block_on(crate::agent::run(rx)).unwrap_or_else(|e| {
            tracing::error!("{e:#}");
            true
        });
        if restart {
            // Exit without reporting "stopped" so the recovery actions restart us.
            std::process::exit(1);
        }
        status.set_service_status(set(ServiceState::Stopped, ServiceControlAccept::empty()))?;
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn refresh_definition() {}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn install(_p: Priority) -> Result<()> {
    bail!("Installing as a service isn't supported on this OS yet; use `opendirectory-agent run`")
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn uninstall() -> Result<()> {
    Ok(())
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn status() -> String {
    "unsupported".into()
}
