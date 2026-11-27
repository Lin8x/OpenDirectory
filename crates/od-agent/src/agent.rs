//! The check-in loop: report inventory and compliance, receive policy,
//! accounts and jobs, apply them, repeat.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use od_core::protocol::*;
use od_core::{ComplianceItem, JobKind, OsFamily};
use tokio::sync::watch;

use crate::state::AgentState;
use crate::{accounts, certs, enduser, helpdesk, inventory, jobs, laps, policy, policy_scripts};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const PATCH_SCAN_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// The latest patch scan, shared between jobs and the check-in loop.
static PATCHES: std::sync::Mutex<Option<od_core::PatchReport>> = std::sync::Mutex::new(None);

pub fn set_patch_report(r: od_core::PatchReport) {
    *PATCHES.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
}
fn patch_report() -> Option<od_core::PatchReport> {
    PATCHES.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
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
        laps: None,
        cert_not_after: None,
        servers: Vec::new(),
        epoch: 0,
        notice_acks: Default::default(),
    };
    state.save()?;
    Ok(state)
}

/// Run until `shutdown` flips to true. Returns `true` if a restart was requested.
pub async fn run(mut shutdown: watch::Receiver<bool>) -> Result<bool> {
    let mut state = AgentState::load()?.context("This endpoint isn't enrolled yet. Run `opendirectory-agent enroll` first.")?;
    // A just-installed update that keeps failing is rolled back here.
    if crate::updater::on_start() {
        return Ok(true);
    }
    let mut last_update_try: Option<Instant> = None;
    let client = state.client()?;
    let mut device_id = crate::identity::DeviceIdentity::default();
    device_id.init(&client);
    tracing::info!("Endpoint Agent {VERSION} reporting to {}", state.server.url);

    let mut last_inventory: Option<Instant> = None;
    let mut last_apply: Option<Instant> = None;
    let mut last_settings = String::new();
    let mut compliance: Vec<ComplianceItem> = Vec::new();
    let mut account_item: Option<ComplianceItem> = None;
    let mut laps_item: Option<ComplianceItem> = None;
    let mut cert_item: Option<ComplianceItem> = None;
    let mut backoff = Duration::from_secs(5);
    let mut force_inventory = false;
    let mut logons = policy_scripts::LogonWatcher::default();
    let mut last_patch_scan: Option<Instant> = None;
    // Long poll so jobs and shells start right away instead of at the next check-in.
    let wake = std::sync::Arc::new(tokio::sync::Notify::new());
    {
        let (c, w) = (client.clone(), wake.clone());
        tokio::spawn(async move {
            loop {
                match c.get::<WaitResponse>("/agent/wait").await {
                    Ok(v) => {
                        // Management queries are answered right away, without a check-in.
                        for q in v.queries {
                            crate::manage::spawn_query(q, c.clone());
                        }
                        if v.wake {
                            w.notify_one();
                        }
                    }
                    Err(_) => tokio::time::sleep(Duration::from_secs(30)).await,
                }
            }
        });
    }
    let mut sent_patches: Option<od_core::PatchReport> = None;
    let mut notice = enduser::NoticeWatcher::default();
    let mut notice_item: Option<ComplianceItem> = None;
    let mut help_desk = helpdesk::HelpDesk::default();

    loop {
        // Device certificate for the Internet Gateway (needed before a
        // device enrolled over the internet can check in).
        device_id.maintain(&client).await;
        // Scan for OS updates in the background every few hours.
        if last_patch_scan.map(|t| t.elapsed() > PATCH_SCAN_EVERY).unwrap_or(true) {
            last_patch_scan = Some(Instant::now());
            tokio::spawn(async { set_patch_report(crate::patches::scan().await) });
        }
        let patches_now = patch_report();
        let patches_changed = patches_now != sent_patches;
        let send_inventory = force_inventory || patches_changed || last_inventory.map(|t| t.elapsed() > INVENTORY_EVERY).unwrap_or(true);
        let mut all_compliance = compliance.clone();
        all_compliance.extend(account_item.clone());
        all_compliance.extend(laps_item.clone());
        all_compliance.extend(cert_item.clone());
        all_compliance.extend(notice_item.clone());
        let req = CheckinRequest {
            agent_version: VERSION.into(),
            inventory: if send_inventory {
                let mut inv = inventory::collect().await;
                inv.patches = patches_now.clone();
                Some(inv)
            } else {
                None
            },
            compliance: all_compliance,
            account_versions: state.accounts.iter().map(|(k, v)| (k.clone(), v.password_version)).collect(),
            known_epoch: state.epoch,
            platform: crate::updater::PLATFORM.into(),
            chat_since: help_desk.since(),
        };

        let wait = match client.post::<CheckinResponse, _>("/agent/checkin", &req).await {
            Ok(resp) if resp.epoch < state.epoch => {
                // A server that was primary before a replica was promoted.
                tracing::warn!("{} is out of date (a replica was promoted); switching servers", client.base_url());
                client.fail_over();
                Duration::from_secs(5)
            }
            Ok(resp) => {
                backoff = Duration::from_secs(5);
                crate::updater::confirm();
                // Agent updates offered by the Management Server.
                if let Some(u) = &resp.update {
                    if last_update_try.map(|t| t.elapsed() > Duration::from_secs(1800)).unwrap_or(true) {
                        last_update_try = Some(Instant::now());
                        match crate::updater::apply(u, &client).await {
                            Ok(true) => return Ok(true),
                            Ok(false) => {}
                            Err(e) => tracing::warn!("agent update to {} failed: {e:#}", u.version),
                        }
                    }
                }
                // Replica servers: remember every server for failover, the
                // current primary first.
                if !resp.servers.is_empty() {
                    let mut list = resp.servers.clone();
                    list.sort_by_key(|s| !s.primary);
                    let targets: Vec<od_client::ServerTarget> =
                        list.iter().map(|s| od_client::ServerTarget { url: s.url.clone(), fingerprint: s.fingerprint.clone() }).collect();
                    if targets != state.servers || resp.epoch != state.epoch {
                        state.servers = targets.clone();
                        state.epoch = resp.epoch;
                        client.set_servers(&targets);
                        if let Err(e) = state.save() {
                            tracing::error!("saving agent state: {e:#}");
                        }
                    }
                }
                if send_inventory {
                    last_inventory = Some(Instant::now());
                    force_inventory = false;
                    sent_patches = patches_now.clone();
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

                // LAPS: rotate the local admin password when due or requested.
                let (item, changed) = laps::maintain(&resp.settings, resp.laps_rotate, &mut state, &client).await;
                laps_item = item;
                let (item, cert_changed) = certs::maintain(&resp.settings, &mut state, &client).await;
                cert_item = item;
                if changed || cert_changed {
                    if let Err(e) = state.save() {
                        tracing::error!("saving agent state: {e:#}");
                    }
                }

                // GPO startup, sign-in and scheduled scripts.
                if policy_scripts::run_due(&resp.scripts, &mut state.script_runs, &mut logons, &client).await {
                    if let Err(e) = state.save() {
                        tracing::error!("saving agent state: {e:#}");
                    }
                }

                // End-user experience: managed-device notice, help desk, and
                // the rules remote sessions must follow.
                let session_rules = enduser::SessionRules::from_settings(&resp.settings);
                crate::manage::set_settings(&resp.settings);
                let (item, acks_changed) = notice.maintain(&resp.settings, &mut state.notice_acks).await;
                notice_item = item;
                if acks_changed {
                    if let Err(e) = state.save() {
                        tracing::error!("saving agent state: {e:#}");
                    }
                }
                help_desk.sync(&resp.settings, resp.chat, &client).await;

                // Remote shells an admin opened.
                for req in resp.shells {
                    crate::shell::spawn(req, client.clone(), session_rules);
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
            _ = wake.notified() => {}
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    tracing::info!("Endpoint Agent stopping");
                    return Ok(false);
                }
            }
        }
    }
}
