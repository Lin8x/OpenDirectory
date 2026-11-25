//! The agent's on-disk configuration and state.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use od_client::ServerTarget;
use serde::{Deserialize, Serialize};

pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("OD_AGENT_DIR") {
        return PathBuf::from(d);
    }
    if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into())).join("OpenDirectory").join("agent")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/Library/Application Support/OpenDirectory/agent")
    } else {
        PathBuf::from("/var/lib/opendirectory-agent")
    }
}

/// A local account this agent created and therefore manages.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ManagedAccountState {
    pub password_version: i64,
    pub admin: bool,
    pub enabled: bool,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub server: ServerTarget,
    pub endpoint_id: String,
    pub endpoint_secret: String,
    pub enrolled_at: chrono::DateTime<chrono::Utc>,
    /// Accounts created by OpenDirectory. Accounts not listed here are never touched.
    #[serde(default)]
    pub accounts: BTreeMap<String, ManagedAccountState>,
    /// When each GPO script last ran, keyed by script id and trigger.
    #[serde(default)]
    pub script_runs: BTreeMap<String, ScriptRunState>,
    /// The LAPS account this agent manages, if any.
    #[serde(default)]
    pub laps: Option<LapsState>,
    /// The auto-enrolled machine certificate's expiry.
    #[serde(default)]
    pub cert_not_after: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LapsState {
    pub account: String,
    pub rotated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ScriptRunState {
    pub revision: String,
    pub last_run: Option<chrono::DateTime<chrono::Utc>>,
    /// Boot time (unix minutes) of the last startup run.
    #[serde(default)]
    pub boot: i64,
}

impl AgentState {
    pub fn path() -> PathBuf {
        data_dir().join("agent.json")
    }

    pub fn load() -> Result<Option<AgentState>> {
        let p = Self::path();
        if !p.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
        Ok(Some(serde_json::from_str(&raw).with_context(|| format!("parse {}", p.display()))?))
    }

    pub fn save(&self) -> Result<()> {
        let p = Self::path();
        std::fs::create_dir_all(p.parent().unwrap())?;
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, &p)?;
        Ok(())
    }

    pub fn client(&self) -> Result<od_client::Client> {
        Ok(od_client::Client::new(&self.server)?.with_endpoint(&self.endpoint_id, &self.endpoint_secret))
    }
}
