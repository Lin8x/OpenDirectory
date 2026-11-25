//! Endpoint Agent gateway: enrollment, check-in and job results.

use std::collections::HashMap;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Path, State};
use axum::Json;
use chrono::Utc;
use od_core::policy::{effective_settings, Setting};
use od_core::protocol::*;
use od_core::Endpoint;
use uuid::Uuid;

use crate::auth::EndpointCaller;
use crate::crypto::{random_token, sha256_hex};
use crate::error::{ApiError, ApiResult};
use crate::{AppState, CHECKIN_INTERVAL_SECS};

pub async fn enroll(
    State(s): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<EnrollRequest>,
) -> ApiResult<Json<EnrollResponse>> {
    let token =
        s.db.consume_token(req.token.trim())?.ok_or_else(|| ApiError::forbidden("That enrollment token is invalid, expired or used up"))?;
    let secret = random_token(32);
    let hostname = if req.inventory.hostname.trim().is_empty() { "unnamed-endpoint".to_string() } else { req.inventory.hostname.clone() };
    let e = Endpoint {
        id: Uuid::new_v4(),
        hostname: hostname.clone(),
        os: req.os,
        ou_id: token.ou_id,
        notes: String::new(),
        agent_version: req.agent_version,
        enrolled_at: Utc::now(),
        last_seen_at: Some(Utc::now()),
        online: true,
        inventory: req.inventory,
        compliance: vec![],
        public_ip: peer.ip().to_string(),
    };
    s.db.insert_endpoint(&e, &sha256_hex(secret.as_bytes()))?;
    s.db.audit("Endpoint Agent", "endpoint.enroll", &hostname, &format!("{} via token \"{}\"", e.os.label(), token.description));
    Ok(Json(EnrollResponse { endpoint_id: e.id, endpoint_secret: secret, checkin_interval_secs: CHECKIN_INTERVAL_SECS }))
}

pub async fn checkin(
    State(s): State<AppState>,
    EndpointCaller(auth): EndpointCaller,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<CheckinRequest>,
) -> ApiResult<Json<CheckinResponse>> {
    s.db.record_checkin(auth.id, &req.agent_version, req.inventory.as_ref(), &req.compliance, &peer.ip().to_string())?;

    let ou_parent: HashMap<Uuid, Option<Uuid>> = s.db.list_ous()?.into_iter().map(|o| (o.id, o.parent_id)).collect();
    let eff = effective_settings(&s.db.list_policies()?, auth.ou_id, &ou_parent);

    // Resolve sign-in account settings into concrete accounts.
    let mut accounts: HashMap<Uuid, ManagedAccount> = HashMap::new();
    let groups = s.db.list_groups()?;
    let users: HashMap<Uuid, od_core::User> = s.db.list_users()?.into_iter().map(|u| (u.id, u)).collect();
    let mut settings = Vec::new();
    for es in eff {
        match es.setting {
            Setting::LocalAccounts { group_id, admin } => {
                let Some(g) = groups.iter().find(|g| g.id == group_id) else { continue };
                for uid in &g.member_ids {
                    let Some(u) = users.get(uid) else { continue };
                    let secrets = s.db.user_secrets(u.id)?;
                    let Some(sealed) = secrets.password_sealed else { continue };
                    let entry = accounts.entry(u.id).or_insert_with(|| ManagedAccount {
                        username: u.username.clone(),
                        display_name: u.display_name.clone(),
                        admin: false,
                        enabled: u.enabled,
                        password_version: secrets.password_version,
                        password: None,
                    });
                    entry.admin |= admin;
                    let applied = req.account_versions.get(&u.username).copied().unwrap_or(0);
                    if applied != secrets.password_version && entry.password.is_none() {
                        entry.password = Some(String::from_utf8(s.sealer.open(&sealed)?).map_err(|e| anyhow::anyhow!(e))?);
                    }
                }
            }
            other => settings.push(other),
        }
    }

    let jobs = s.db.claim_jobs(auth.id)?.into_iter().map(|j| AgentJob { id: j.id, kind: j.kind }).collect();
    let mut accounts: Vec<ManagedAccount> = accounts.into_values().collect();
    accounts.sort_by(|a, b| a.username.cmp(&b.username));
    Ok(Json(CheckinResponse {
        checkin_interval_secs: CHECKIN_INTERVAL_SECS,
        jobs,
        settings,
        accounts,
        want_inventory: auth.want_inventory,
    }))
}

pub async fn job_result(
    State(s): State<AppState>,
    EndpointCaller(auth): EndpointCaller,
    Path(id): Path<Uuid>,
    Json(res): Json<JobResult>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut output = res.output;
    if output.len() > 1024 * 1024 {
        let cut = output.char_indices().map(|(i, _)| i).take_while(|i| *i <= 1024 * 1024).last().unwrap_or(0);
        output.truncate(cut);
        output.push_str("\n… output truncated at 1 MB");
    }
    if !s.db.finish_job(id, auth.id, res.success, res.exit_code, &output)? {
        return Err(ApiError::not_found("Running job"));
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}
