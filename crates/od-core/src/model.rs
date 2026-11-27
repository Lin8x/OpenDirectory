use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Operating-system families the agent supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OsFamily {
    Windows,
    Macos,
    Linux,
    /// Phones and tablets running the OpenDirectory Android agent.
    Android,
}

impl OsFamily {
    pub const ALL: [OsFamily; 4] = [OsFamily::Windows, OsFamily::Macos, OsFamily::Linux, OsFamily::Android];

    pub fn current() -> OsFamily {
        if cfg!(target_os = "windows") {
            OsFamily::Windows
        } else if cfg!(target_os = "macos") {
            OsFamily::Macos
        } else {
            OsFamily::Linux
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OsFamily::Windows => "Windows",
            OsFamily::Macos => "macOS",
            OsFamily::Linux => "Linux",
            OsFamily::Android => "Android",
        }
    }
}

/// An organizational unit: a folder for users, groups and endpoints, exactly
/// like an OU in Active Directory. Policies are linked to OUs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgUnit {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub parent_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Can sign in to endpoints but not to the admin console.
    #[default]
    User,
    /// Full control of the directory (like Domain Admins).
    Admin,
}

/// A directory user as returned by the API. Secrets never leave the server
/// through this type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub title: String,
    pub department: String,
    pub ou_id: Option<Uuid>,
    pub enabled: bool,
    pub role: Role,
    pub must_change_password: bool,
    pub password_set_at: Option<DateTime<Utc>>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub group_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub ou_id: Option<Uuid>,
    pub member_ids: Vec<Uuid>,
    /// Computer members (endpoints), like computer accounts in an AD group.
    #[serde(default)]
    pub endpoint_ids: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Disk {
    pub mount: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub file_system: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetInterface {
    pub name: String,
    pub mac: String,
    pub ips: Vec<String>,
}

/// Everything the agent reports about a machine.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Inventory {
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub os_edition: String,
    pub kernel_version: String,
    pub arch: String,
    pub manufacturer: String,
    pub model: String,
    pub serial_number: String,
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub memory_bytes: u64,
    pub uptime_secs: u64,
    pub disks: Vec<Disk>,
    pub interfaces: Vec<NetInterface>,
    pub logged_in_users: Vec<String>,
    pub disk_encrypted: Option<bool>,
    pub firewall_enabled: Option<bool>,
    /// Pending OS updates from the last scan.
    #[serde(default)]
    pub patches: Option<PatchReport>,
}

/// One pending OS update.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Patch {
    pub title: String,
    /// KB number, package version or update label.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub security: bool,
    /// Critical, Important, Moderate, Low, or empty.
    #[serde(default)]
    pub severity: String,
}

/// The result of a patch scan on an endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PatchReport {
    pub scanned_at: Option<DateTime<Utc>>,
    pub pending: Vec<Patch>,
    pub reboot_required: bool,
    /// apt, dnf, Windows Update, softwareupdate…
    #[serde(default)]
    pub source: String,
    /// Why the scan couldn't run, if it couldn't.
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplianceState {
    Compliant,
    Applied,
    NonCompliant,
    Failed,
    Unsupported,
}

/// The outcome of applying one policy setting on one endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceItem {
    pub setting: String,
    pub state: ComplianceState,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: Uuid,
    pub hostname: String,
    pub os: OsFamily,
    pub ou_id: Option<Uuid>,
    pub notes: String,
    pub agent_version: String,
    pub enrolled_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub online: bool,
    pub inventory: Inventory,
    pub compliance: Vec<ComplianceItem>,
    pub public_ip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentToken {
    pub token: String,
    pub description: String,
    pub ou_id: Option<Uuid>,
    pub uses: u32,
    pub max_uses: Option<u32>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Shell {
    /// `sh` on macOS/Linux, `cmd` on Windows.
    Default,
    Powershell,
    Bash,
    Zsh,
    Cmd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JobKind {
    Script {
        shell: Shell,
        script: String,
        /// Extra environment variables, e.g. `OD_PARAM_*` script parameters.
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        env: std::collections::BTreeMap<String, String>,
    },
    RefreshInventory,
    RestartAgent,
    /// Check for OS updates now.
    ScanUpdates,
    /// Install pending OS updates.
    InstallUpdates {
        #[serde(default)]
        security_only: bool,
        /// Restart afterwards if an update needs it.
        #[serde(default)]
        reboot: bool,
    },
    /// Show a message to whoever is signed in (like `msg` on Windows).
    NotifyUser {
        title: String,
        message: String,
    },
    /// Manage a service, package, process or container (see [`crate::manage`]).
    Manage {
        action: crate::manage::ManageAction,
    },
    /// A job type from a newer version.
    #[serde(other)]
    Unknown,
}

impl JobKind {
    /// The oldest Endpoint Agent version that can run this job.
    pub fn min_agent_version(&self) -> &'static str {
        match self {
            JobKind::ScanUpdates | JobKind::InstallUpdates { .. } => "0.5.0",
            JobKind::NotifyUser { .. } => "0.13.0",
            JobKind::Manage { .. } => crate::manage::MIN_AGENT,
            _ => "0.0.0",
        }
    }
}

impl JobKind {
    pub fn summary(&self) -> String {
        match self {
            JobKind::Script { script, .. } => {
                let first = script.lines().next().unwrap_or("").trim();
                if first.len() > 60 {
                    format!("Script: {}…", &first[..first.char_indices().nth(60).map(|c| c.0).unwrap_or(first.len())])
                } else {
                    format!("Script: {first}")
                }
            }
            JobKind::RefreshInventory => "Refresh inventory".into(),
            JobKind::RestartAgent => "Restart agent".into(),
            JobKind::ScanUpdates => "Scan for updates".into(),
            JobKind::InstallUpdates { security_only: true, .. } => "Install security updates".into(),
            JobKind::InstallUpdates { .. } => "Install updates".into(),
            JobKind::NotifyUser { title, .. } => format!("Message the user: {title}"),
            JobKind::Manage { action } => action.summary(),
            JobKind::Unknown => "Unknown job type".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub endpoint_id: Uuid,
    pub kind: JobKind,
    pub status: JobStatus,
    /// Library script this run came from, if any.
    #[serde(default)]
    pub script_id: Option<Uuid>,
    /// Runs started together (one script against many endpoints) share a batch.
    #[serde(default)]
    pub batch_id: Option<Uuid>,
    pub exit_code: Option<i32>,
    pub output: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub details: String,
}
