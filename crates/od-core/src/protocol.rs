//! Messages exchanged between the agent and the server.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{ComplianceItem, Inventory, JobKind, OsFamily};
use crate::policy::Setting;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollRequest {
    pub token: String,
    pub os: OsFamily,
    pub agent_version: String,
    pub inventory: Inventory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollResponse {
    pub endpoint_id: Uuid,
    /// Long random secret the agent presents on every request.
    pub endpoint_secret: String,
    pub checkin_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckinRequest {
    pub agent_version: String,
    /// Full inventory, sent when it changed or was requested.
    pub inventory: Option<Inventory>,
    pub compliance: Vec<ComplianceItem>,
    /// Password version the agent last applied for each managed account.
    #[serde(default)]
    pub account_versions: std::collections::HashMap<String, i64>,
}

/// A local sign-in account the agent must keep in sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedAccount {
    pub username: String,
    pub display_name: String,
    pub admin: bool,
    pub enabled: bool,
    pub password_version: i64,
    /// Only sent when the endpoint's applied version is older.
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentJob {
    pub id: Uuid,
    pub kind: JobKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckinResponse {
    pub checkin_interval_secs: u64,
    pub jobs: Vec<AgentJob>,
    /// Effective settings for this endpoint (accounts are resolved separately).
    pub settings: Vec<Setting>,
    pub accounts: Vec<ManagedAccount>,
    /// Ask the agent to send full inventory next time.
    pub want_inventory: bool,
    /// GPO startup, sign-in and scheduled scripts for this endpoint.
    #[serde(default)]
    pub scripts: Vec<crate::scripts::PolicyScript>,
    /// An admin asked for the LAPS password to be rotated now.
    #[serde(default)]
    pub laps_rotate: bool,
    /// Interactive shells an admin opened; the agent connects back for each.
    #[serde(default)]
    pub shells: Vec<ShellRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellRequest {
    pub session_id: Uuid,
    pub cols: u16,
    pub rows: u16,
    /// Connect this local TCP port instead of starting a shell.
    #[serde(default)]
    pub tunnel_port: Option<u16>,
}

/// Certificate auto-enrollment: the agent sends a CSR for its own key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertEnrollRequest {
    pub csr_pem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertEnrollResponse {
    pub cert_pem: String,
    /// The issuing CA certificate.
    pub ca_pem: String,
    pub not_after: chrono::DateTime<chrono::Utc>,
}

/// The agent escrows a new LAPS password before it sets it locally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LapsEscrow {
    pub account: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    pub exit_code: Option<i32>,
    pub output: String,
    pub success: bool,
}
