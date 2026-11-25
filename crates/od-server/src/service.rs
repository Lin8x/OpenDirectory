//! Running the Management Server as an always-on system service: a systemd
//! unit on Linux, a launchd daemon on macOS and a Windows Service. Each starts
//! at boot (before anyone signs in) and restarts if it stops.

#[allow(unused_imports)]
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

#[cfg_attr(not(windows), allow(dead_code))]
pub const SERVICE_NAME: &str = "OpenDirectoryServer";
pub const DISPLAY_NAME: &str = "OpenDirectory Management Server";

/// Arguments the service is started with.
pub struct ServiceArgs {
    pub data_dir: PathBuf,
    pub listen: String,
    pub public_host: Option<String>,
}

impl ServiceArgs {
    #[allow(dead_code)]
    fn argv(&self, last: &str) -> Vec<String> {
        let mut v = vec!["--data-dir".to_string(), self.data_dir.display().to_string(), "--listen".into(), self.listen.clone()];
        if let Some(h) = &self.public_host {
            v.push("--public-host".into());
            v.push(h.clone());
        }
        v.push(last.into());
        v
    }
}

fn install_path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into()))
            .join("OpenDirectory")
            .join("opendirectory-server.exe")
    } else {
        PathBuf::from("/usr/local/bin/opendirectory-server")
    }
}

/// Copy the running binary to the standard location.
#[allow(dead_code)]
fn copy_self() -> Result<PathBuf> {
    let me = std::env::current_exe()?.canonicalize()?;
    let dest = install_path();
    if dest.exists() && dest.canonicalize().ok() == Some(me.clone()) {
        return Ok(dest);
    }
    std::fs::create_dir_all(dest.parent().expect("has parent"))?;
    let tmp = dest.with_extension("new");
    std::fs::copy(&me, &tmp).with_context(|| format!("copy server to {}", dest.display()))?;
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

#[allow(dead_code)]
fn sh(program: &str, args: &[&str]) -> Result<std::process::Output> {
    std::process::Command::new(program).args(args).output().with_context(|| format!("run {program}"))
}

// ---------------------------------------------------------------- Linux
#[cfg(target_os = "linux")]
const UNIT_PATH: &str = "/etc/systemd/system/opendirectory-server.service";

#[cfg(target_os = "linux")]
pub fn install(a: &ServiceArgs) -> Result<()> {
    require_root()?;
    let bin = copy_self()?;
    let args: Vec<String> = a.argv("run").into_iter().map(|x| if x.contains(' ') { format!("\"{x}\"") } else { x }).collect();
    let unit = format!(
        "[Unit]\nDescription={DISPLAY_NAME}\nDocumentation=https://github.com/Lin8x/OpenDirectory\nAfter=network-online.target\nWants=network-online.target\n\n\
         [Service]\nType=simple\nExecStart={} {}\nRestart=always\nRestartSec=5\nLimitNOFILE=65536\n\n[Install]\nWantedBy=multi-user.target\n",
        bin.display(),
        args.join(" ")
    );
    std::fs::write(UNIT_PATH, unit)?;
    sh("systemctl", &["daemon-reload"])?;
    let o = sh("systemctl", &["enable", "--now", "opendirectory-server.service"])?;
    if !o.status.success() {
        bail!("systemctl enable failed: {}", String::from_utf8_lossy(&o.stderr));
    }
    let _ = sh("systemctl", &["restart", "opendirectory-server.service"]);
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn uninstall() -> Result<()> {
    require_root()?;
    let _ = sh("systemctl", &["disable", "--now", "opendirectory-server.service"]);
    let _ = std::fs::remove_file(UNIT_PATH);
    let _ = sh("systemctl", &["daemon-reload"]);
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn status() -> String {
    sh("systemctl", &["is-active", "opendirectory-server.service"])
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

// ---------------------------------------------------------------- macOS
#[cfg(target_os = "macos")]
const PLIST_PATH: &str = "/Library/LaunchDaemons/dev.opendirectory.server.plist";

#[cfg(target_os = "macos")]
pub fn install(a: &ServiceArgs) -> Result<()> {
    require_root()?;
    let bin = copy_self()?;
    std::fs::create_dir_all("/Library/Logs/OpenDirectory")?;
    let args: String = std::iter::once(bin.display().to_string())
        .chain(a.argv("run"))
        .map(|x| format!("<string>{}</string>", x.replace('&', "&amp;").replace('<', "&lt;")))
        .collect();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.opendirectory.server</string>
  <key>ProgramArguments</key><array>{args}</array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>5</integer>
  <key>StandardOutPath</key><string>/Library/Logs/OpenDirectory/server.log</string>
  <key>StandardErrorPath</key><string>/Library/Logs/OpenDirectory/server.log</string>
</dict>
</plist>
"#
    );
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
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn status() -> String {
    match sh("launchctl", &["print", "system/dev.opendirectory.server"]) {
        Ok(o) if o.status.success() => {
            if String::from_utf8_lossy(&o.stdout).contains("state = running") {
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
pub fn install(a: &ServiceArgs) -> Result<()> {
    use std::ffi::OsString;
    use windows_service::service::{ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceType};
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    let bin = copy_self()?;
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
        launch_arguments: a.argv("windows-service").into_iter().map(OsString::from).collect(),
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let svc = mgr.create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)?;
    svc.set_description("OpenDirectory directory, Group Policy and endpoint management server.")?;
    let _ = sh("sc.exe", &["failure", SERVICE_NAME, "reset=", "86400", "actions=", "restart/5000/restart/5000/restart/30000"]);
    let port = a.listen.rsplit(':').next().unwrap_or("8443").to_string();
    let _ = sh(
        "netsh.exe",
        &[
            "advfirewall",
            "firewall",
            "add",
            "rule",
            "name=OpenDirectory Management Server",
            "dir=in",
            "action=allow",
            "protocol=TCP",
            &format!("localport={port}"),
        ],
    );
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
    let _ = sh("netsh.exe", &["advfirewall", "firewall", "delete", "rule", "name=OpenDirectory Management Server"]);
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
    use std::sync::OnceLock;
    use std::time::Duration;

    use anyhow::Result;
    use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType};
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::{define_windows_service, service_dispatcher};

    use super::SERVICE_NAME;

    static CONFIG: OnceLock<crate::ServerConfig> = OnceLock::new();

    define_windows_service!(ffi_service_main, service_main);

    pub fn run_dispatcher(cfg: crate::ServerConfig) -> Result<()> {
        let _ = CONFIG.set(cfg);
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
        Ok(())
    }

    fn service_main(_args: Vec<OsString>) {
        if let Err(e) = run_service() {
            tracing::error!("service failed: {e:#}");
        }
    }

    fn run_service() -> Result<()> {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let handler = move |event| match event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = tx.send(());
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
        let rt = tokio::runtime::Runtime::new()?;
        let cfg = CONFIG.get().cloned().expect("config set before dispatch");
        let server = rt.block_on(crate::start(cfg))?;
        status.set_service_status(set(ServiceState::Running, ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN))?;
        let _ = rx.recv();
        server.shutdown();
        status.set_service_status(set(ServiceState::Stopped, ServiceControlAccept::empty()))?;
        Ok(())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn install(_: &ServiceArgs) -> Result<()> {
    bail!("Installing as a service isn't supported on this OS yet; run `opendirectory-server run`")
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn uninstall() -> Result<()> {
    Ok(())
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn status() -> String {
    "unsupported".into()
}
