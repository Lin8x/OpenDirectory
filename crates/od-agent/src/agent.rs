//! The check-in loop: report inventory and compliance, receive policy,
//! accounts and jobs, apply them, repeat.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use od_core::protocol::*;
use od_core::{ComplianceItem, JobKind, OsFamily};
use tokio::sync::watch;

use crate::state::AgentState;
use crate::{accounts, inventory, jobs, policy, policy_scripts};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const INVENTORY_EVERY: Duration = Duration::from_secs(60 * 60);
const REAPPLY_EVERY: Duration = Duration::from_secs(15 * 60);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

pub async fn enroll(server: &str, token: &str, fingerprint: Option<String>) -> Result<AgentState> {
    let target = od_client::ServerTarget { url: server.trim_end_matches('/').to_string(), fingerprint };
    let client = od_client::Client::new(&target)?;
    let inv = inventory::collect().await;
    let req = EnrollRequest { token: token.trim().to_uppercase(), os: OsFamily::current(), agent_version: VERSION.into(), inventory: inv };
    let resp: EnrollResponse = client.post("/agent/enroll", &req).await.context("enrollment failed")?;
    let state = AgentState {
        server: target,
        endpoint_id: resp.endpoint_id.to_string(),
        endpoint_secret: resp.endpoint_secret,
        enrolled_at: chrono::Utc::now(),
        accounts: Default::default(),
        script_runs: Default::default(),
    };
    state.save()?;
    Ok(state)
}

/// Run until `shutdown` flips to true. Returns `true` if a restart was requested.
pub async fn run(mut shutdown: watch::Receiver<bool>) -> Result<bool> {
    let mut state = AgentState::load()?.context("This endpoint isn't enrolled yet. Run `opendirectory-agent enroll` first.")?;
    let client = state.client()?;
    tracing::info!("Endpoint Agent {VERSION} reporting to {}", state.server.url);

    let mut last_inventory: Option<Instant> = None;
    let mut last_apply: Option<Instant> = None;
    let mut last_settings = String::new();
    let mut compliance: Vec<ComplianceItem> = Vec::new();
    let mut account_item: Option<ComplianceItem> = None;
    let mut backoff = Duration::from_secs(5);
    let mut force_inventory = false;
    let mut logons = policy_scripts::LogonWatcher::default();

    loop {
        let send_inventory = force_inventory || last_inventory.map(|t| t.elapsed() > INVENTORY_EVERY).unwrap_or(true);
        let mut all_compliance = compliance.clone();
        all_compliance.extend(account_item.clone());
        let req = CheckinRequest {
            agent_version: VERSION.into(),
            inventory: if send_inventory { Some(inventory::collect().await) } else { None },
            compliance: all_compliance,
            account_versions: state.accounts.iter().map(|(k, v)| (k.clone(), v.password_version)).collect(),
        };

        let wait = match client.post::<CheckinResponse, _>("/agent/checkin", &req).await {
            Ok(resp) => {
                backoff = Duration::from_secs(5);
                if send_inventory {
                    last_inventory = Some(Instant::now());
                    force_inventory = false;
                }
                force_inventory |= resp.want_inventory;

                // Sign-in accounts.
                let before = state.accounts.clone();
                account_item = accounts::sync(&resp.accounts, &mut state.accounts).await;
                if state.accounts != before {
                    if let Err(e) = state.save() {
                        tracing::error!("saving agent state: {e:#}");
                    }
                }

                // Policy: apply when it changes, and periodically to correct drift.
                let fingerprint = serde_json::to_string(&resp.settings).unwrap_or_default();
                if fingerprint != last_settings || last_apply.map(|t| t.elapsed() > REAPPLY_EVERY).unwrap_or(true) {
                    compliance = policy::apply_all(&resp.settings).await;
                    last_settings = fingerprint;
                    last_apply = Some(Instant::now());
                }

                // GPO startup, sign-in and scheduled scripts.
                if policy_scripts::run_due(&resp.scripts, &mut state.script_runs, &mut logons, &client).await {
                    if let Err(e) = state.save() {
                        tracing::error!("saving agent state: {e:#}");
                    }
                }

                // Jobs run concurrently; each reports its own result.
                let mut restart = false;
                for job in resp.jobs {
                    if matches!(job.kind, JobKind::RefreshInventory) {
                        force_inventory = true;
                    }
                    restart |= matches!(job.kind, JobKind::RestartAgent);
                    let c = client.clone();
                    let handle = tokio::spawn(async move {
                        let result = jobs::run(&job).await;
                        if let Err(e) = c.post::<serde_json::Value, _>(&format!("/agent/jobs/{}/result", job.id), &result).await {
                            tracing::warn!("reporting job {}: {e}", job.id);
                        }
                    });
                    if restart {
                        let _ = handle.await;
                    }
                }
                if restart {
                    tracing::info!("Restart requested from the Admin Center");
                    return Ok(true);
                }
                Duration::from_secs(resp.checkin_interval_secs.clamp(5, 3600))
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("401") || msg.contains("Sign in required") {
                    tracing::error!(
                        "The Management Server no longer recognizes this endpoint (it may have been retired). Re-enroll to continue."
                    );
                } else {
                    tracing::warn!("check-in failed: {msg}");
                }
                let w = backoff;
                backoff = (backoff * 2).min(MAX_BACKOFF);
                w
            }
        };

        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    tracing::info!("Endpoint Agent stopping");
                    return Ok(false);
                }
            }
        }
    }
}
