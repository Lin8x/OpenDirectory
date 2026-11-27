//! Per-endpoint management from the Admin Center: services, installed
//! software, processes and containers.
//!
//! Reads ("list the services") are *queries*: the server hands them to the
//! agent over the wake-up long poll and waits for the answer, so nothing is
//! stored. Changes ("restart nginx") are *actions*: they run as jobs, so they
//! show in the job history with their output, and they're audit-logged.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The oldest Endpoint Agent that understands queries, actions and container exec.
pub const MIN_AGENT: &str = "0.14.0";

/// Something to read from an endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ManageQuery {
    /// Windows services (SCM), systemd units or launchd jobs.
    Services,
    /// Installed applications and packages, with available updates.
    Software,
    /// Running processes (Task Manager / top).
    Processes,
    /// Docker or Podman containers and images.
    Containers,
    /// The last lines of a container's log.
    ContainerLogs {
        id: String,
        #[serde(default = "tail")]
        tail: u32,
    },
    /// A newer query this agent doesn't know.
    #[serde(other)]
    Unknown,
}
fn tail() -> u32 {
    500
}

impl ManageQuery {
    pub fn area(&self) -> Option<Area> {
        Some(match self {
            ManageQuery::Services => Area::Services,
            ManageQuery::Software => Area::Software,
            ManageQuery::Processes => Area::Processes,
            ManageQuery::Containers | ManageQuery::ContainerLogs { .. } => Area::Containers,
            ManageQuery::Unknown => return None,
        })
    }
}

/// The management areas, each with its own GPO switch and delegated permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    Services,
    Software,
    Processes,
    Containers,
}

impl Area {
    pub fn label(self) -> &'static str {
        match self {
            Area::Services => "Service management",
            Area::Software => "Software management",
            Area::Processes => "Process management",
            Area::Containers => "Container management",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceControl {
    Start,
    Stop,
    Restart,
}

/// Startup type, in Windows SCM terms. On systemd: automatic = enabled,
/// manual = disabled (can still be started), disabled = masked. On launchd:
/// automatic = enabled, disabled = disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupType {
    Automatic,
    /// Automatic (Delayed Start), Windows only.
    AutomaticDelayed,
    Manual,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageOp {
    Install,
    /// Upgrade to the newest version.
    Update,
    /// Uninstall.
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerControl {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Remove,
    /// Pull the image again and recreate the container with the same settings
    /// (`docker compose up -d --pull always` for Compose services).
    Update,
}

/// A change to make on an endpoint. Runs as a job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ManageAction {
    Service {
        name: String,
        control: ServiceControl,
    },
    ServiceStartup {
        name: String,
        startup: StartupType,
    },
    Package {
        operation: PackageOp,
        /// Package id (winget id, apt/dnf/brew name, flatpak app id…).
        name: String,
        /// winget, apt, dnf, zypper, pacman, brew, brew_cask, flatpak, snap.
        /// Empty picks the OS default.
        #[serde(default)]
        manager: String,
    },
    /// End a process (and optionally its child processes).
    KillProcess {
        pid: u32,
        #[serde(default)]
        tree: bool,
        /// The name the admin saw, so a reused PID isn't killed by mistake.
        #[serde(default)]
        expect_name: String,
    },
    Container {
        id: String,
        control: ContainerControl,
    },
    PullImage {
        image: String,
    },
    RemoveImage {
        id: String,
    },
    /// Remove stopped containers, dangling images and unused networks.
    Prune,
    #[serde(other)]
    Unknown,
}

impl ManageAction {
    pub fn area(&self) -> Option<Area> {
        Some(match self {
            ManageAction::Service { .. } | ManageAction::ServiceStartup { .. } => Area::Services,
            ManageAction::Package { .. } => Area::Software,
            ManageAction::KillProcess { .. } => Area::Processes,
            ManageAction::Container { .. } | ManageAction::PullImage { .. } | ManageAction::RemoveImage { .. } | ManageAction::Prune => {
                Area::Containers
            }
            ManageAction::Unknown => return None,
        })
    }

    /// Short audit-log action name, e.g. `service.restart`.
    pub fn audit_action(&self) -> String {
        match self {
            ManageAction::Service { control, .. } => format!("service.{}", snake(control)),
            ManageAction::ServiceStartup { .. } => "service.startup".into(),
            ManageAction::Package { operation, .. } => format!("software.{}", snake(operation)),
            ManageAction::KillProcess { .. } => "process.kill".into(),
            ManageAction::Container { control, .. } => format!("container.{}", snake(control)),
            ManageAction::PullImage { .. } => "image.pull".into(),
            ManageAction::RemoveImage { .. } => "image.remove".into(),
            ManageAction::Prune => "container.prune".into(),
            ManageAction::Unknown => "manage.unknown".into(),
        }
    }

    pub fn summary(&self) -> String {
        match self {
            ManageAction::Service { name, control } => {
                format!("{} service {name}", cap(control_word(*control)))
            }
            ManageAction::ServiceStartup { name, startup } => format!("Set {name} startup type to {}", startup_label(*startup)),
            ManageAction::Package { operation, name, manager } => {
                let verb = match operation {
                    PackageOp::Install => "Install",
                    PackageOp::Update => "Update",
                    PackageOp::Remove => "Uninstall",
                };
                if manager.is_empty() {
                    format!("{verb} {name}")
                } else {
                    format!("{verb} {name} ({manager})")
                }
            }
            ManageAction::KillProcess { pid, tree, expect_name } => {
                let what = if expect_name.is_empty() { format!("PID {pid}") } else { format!("{expect_name} (PID {pid})") };
                if *tree {
                    format!("End process tree {what}")
                } else {
                    format!("End process {what}")
                }
            }
            ManageAction::Container { id, control } => {
                let short: String = id.chars().take(24).collect();
                format!("{} container {short}", cap(container_word(*control)))
            }
            ManageAction::PullImage { image } => format!("Pull image {image}"),
            ManageAction::RemoveImage { id } => format!("Remove image {}", id.chars().take(24).collect::<String>()),
            ManageAction::Prune => "Prune unused containers and images".into(),
            ManageAction::Unknown => "Unknown management action".into(),
        }
    }

    /// Reject anything that could be read as a command-line option or that
    /// isn't a plausible name. Arguments are always passed without a shell,
    /// so this is about flags, not injection.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            ManageAction::Service { name, .. } | ManageAction::ServiceStartup { name, .. } => check_name("service name", name, "@.:-_ $"),
            ManageAction::Package { name, manager, .. } => {
                check_name("package name", name, "@.:+-_/~\\{}")?;
                if !manager.is_empty() && !MANAGERS.contains(&manager.as_str()) {
                    return Err(format!("Unknown package manager {manager}"));
                }
                Ok(())
            }
            ManageAction::KillProcess { pid, .. } => {
                if *pid <= 4 {
                    Err("That process is part of the operating system and can't be ended".into())
                } else {
                    Ok(())
                }
            }
            ManageAction::Container { id, .. } | ManageAction::RemoveImage { id } => check_name("container or image id", id, ".:-_/@"),
            ManageAction::PullImage { image } => check_name("image", image, ".:-_/@"),
            ManageAction::Prune => Ok(()),
            ManageAction::Unknown => Err("Unknown management action".into()),
        }
    }
}

/// Package managers the agent knows.
pub const MANAGERS: &[&str] = &["winget", "apt", "dnf", "zypper", "pacman", "brew", "brew_cask", "flatpak", "snap"];

pub fn check_name(what: &str, v: &str, extra: &str) -> Result<(), String> {
    let v = v.trim();
    if v.is_empty() || v.len() > 256 {
        return Err(format!("The {what} is empty or too long"));
    }
    if v.starts_with('-') {
        return Err(format!("The {what} can't start with a dash"));
    }
    if !v.chars().all(|c| c.is_alphanumeric() || extra.contains(c)) {
        return Err(format!("The {what} has characters that aren't allowed"));
    }
    Ok(())
}

fn snake<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}
fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}
fn control_word(c: ServiceControl) -> &'static str {
    match c {
        ServiceControl::Start => "start",
        ServiceControl::Stop => "stop",
        ServiceControl::Restart => "restart",
    }
}
fn container_word(c: ContainerControl) -> &'static str {
    match c {
        ContainerControl::Start => "start",
        ContainerControl::Stop => "stop",
        ContainerControl::Restart => "restart",
        ContainerControl::Pause => "pause",
        ContainerControl::Unpause => "unpause",
        ContainerControl::Remove => "remove",
        ContainerControl::Update => "update (pull and recreate)",
    }
}
pub fn startup_label(s: StartupType) -> &'static str {
    match s {
        StartupType::Automatic => "Automatic",
        StartupType::AutomaticDelayed => "Automatic (Delayed Start)",
        StartupType::Manual => "Manual",
        StartupType::Disabled => "Disabled",
    }
}

// ------------------------------------------------------------------ wire

/// A query the server wants answered, delivered with the agent's long poll.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub id: Uuid,
    pub query: ManageQuery,
}

/// The agent's answer. `data` is one of the report types below.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueryResult {
    pub ok: bool,
    #[serde(default)]
    pub data: serde_json::Value,
    #[serde(default)]
    pub error: String,
}

/// Exec into a container instead of opening a host shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerExec {
    pub container: String,
    /// Shell to run inside it; default `sh` (tries bash first).
    #[serde(default)]
    pub command: String,
}

// --------------------------------------------------------------- reports

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServiceInfo {
    /// Service name / unit name / launchd label.
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    /// running, stopped, paused, starting, stopping, failed, inactive…
    pub state: String,
    /// automatic, automatic_delayed, manual, disabled, static, masked…
    pub startup: String,
    #[serde(default)]
    pub pid: Option<u32>,
    /// Log On As (Windows) / User= (systemd).
    #[serde(default)]
    pub account: String,
    /// The startup type can be changed.
    #[serde(default = "yes")]
    pub configurable: bool,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServiceReport {
    /// scm, systemd or launchd.
    pub manager: String,
    pub services: Vec<ServiceInfo>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SoftwareInfo {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub publisher: String,
    /// Where it came from: winget, programs (Programs and Features), apt,
    /// rpm, pacman, flatpak, snap, brew, brew_cask, app (an .app bundle).
    pub source: String,
    /// The id to pass to the package manager, if it can manage this one.
    #[serde(default)]
    pub package_id: String,
    /// The manager that can update or remove it.
    #[serde(default)]
    pub manager: String,
    /// A newer version, if one is available.
    #[serde(default)]
    pub update: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SoftwareReport {
    pub items: Vec<SoftwareInfo>,
    /// Package managers present on this endpoint, the default first.
    pub managers: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    #[serde(default)]
    pub parent: Option<u32>,
    pub name: String,
    #[serde(default)]
    pub user: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    /// Seconds since the process started.
    #[serde(default)]
    pub run_secs: u64,
    #[serde(default)]
    pub command: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProcessReport {
    pub processes: Vec<ProcessInfo>,
    pub cpu_count: u32,
    pub cpu_percent: f32,
    pub memory_total: u64,
    pub memory_used: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    /// running, exited, paused, created, restarting, dead.
    pub state: String,
    /// "Up 3 hours", "Exited (0) 2 days ago".
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub ports: String,
    #[serde(default)]
    pub created: String,
    /// Compose project, if it's part of one.
    #[serde(default)]
    pub project: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ImageInfo {
    pub id: String,
    pub repository: String,
    pub tag: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub created: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ContainerReport {
    /// docker or podman; empty when neither is installed.
    pub runtime: String,
    #[serde(default)]
    pub version: String,
    pub containers: Vec<ContainerInfo>,
    pub images: Vec<ImageInfo>,
    #[serde(default)]
    pub error: String,
}

/// One app a GPO deploys, updates or removes (Software Installation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageRule {
    /// Package id for the manager (winget id, apt name, brew formula…).
    pub name: String,
    /// present (install if missing), latest (also keep updated) or absent (uninstall).
    #[serde(default = "present")]
    pub ensure: String,
    /// Package manager to use; empty picks the OS default.
    #[serde(default)]
    pub manager: String,
    /// Only on this platform (package names differ per OS). None = all.
    #[serde(default)]
    pub os: Option<crate::OsFamily>,
}
fn present() -> String {
    "present".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_shapes() {
        let a = ManageAction::Service { name: "nginx".into(), control: ServiceControl::Restart };
        assert_eq!(serde_json::to_string(&a).unwrap(), r#"{"op":"service","name":"nginx","control":"restart"}"#);
        assert_eq!(a.audit_action(), "service.restart");
        assert_eq!(a.summary(), "Restart service nginx");
        let q: ManageQuery = serde_json::from_str(r#"{"op":"container_logs","id":"abc"}"#).unwrap();
        assert_eq!(q, ManageQuery::ContainerLogs { id: "abc".into(), tail: 500 });
        let u: ManageAction = serde_json::from_str(r#"{"op":"teleport"}"#).unwrap();
        assert_eq!(u, ManageAction::Unknown);
    }

    #[test]
    fn validation() {
        let p = |n: &str| ManageAction::Package { operation: PackageOp::Install, name: n.into(), manager: String::new() };
        assert!(p("Mozilla.Firefox").validate().is_ok());
        assert!(p("g++").validate().is_ok());
        assert!(p("--force").validate().is_err());
        assert!(p("a;b").validate().is_err());
        let s = ManageAction::Service { name: "Windows Update".into(), control: ServiceControl::Stop };
        assert!(s.validate().is_ok());
        assert!(ManageAction::KillProcess { pid: 1, tree: false, expect_name: String::new() }.validate().is_err());
        assert!(ManageAction::Package { operation: PackageOp::Install, name: "x".into(), manager: "npm".into() }.validate().is_err());
    }
}
