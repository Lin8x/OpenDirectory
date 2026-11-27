//! Endpoints, enrollment tokens and remote scripts (jobs).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{Duration, Utc};
use od_core::*;
use serde::Deserialize;
use uuid::Uuid;

use super::directory::double_option;
use crate::auth::{Admin, Operator};
use crate::crypto::enrollment_code;
use crate::error::{ApiError, ApiResult};
use crate::rbac::Permission;
use crate::AppState;

pub async fn list(State(s): State<AppState>, op: Operator) -> ApiResult<Json<Vec<Endpoint>>> {
    let mut eps = s.db.list_endpoints()?;
    eps.retain(|e| op.access.sees_endpoint(e));
    Ok(Json(eps))
}

/// An endpoint the operator can see, or 404.
pub(crate) fn visible(s: &AppState, op: &Operator, id: Uuid) -> ApiResult<Endpoint> {
    s.db.get_endpoint(id)?.filter(|e| op.access.sees_endpoint(e)).ok_or_else(|| ApiError::not_found("Endpoint"))
}

pub async fn get(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<Endpoint>> {
    Ok(Json(visible(&s, &op, id)?))
}

#[derive(Deserialize)]
pub struct EndpointInput {
    #[serde(default, with = "double_option")]
    ou_id: Option<Option<Uuid>>,
    notes: Option<String>,
}

pub async fn update(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<EndpointInput>,
) -> ApiResult<Json<Endpoint>> {
    let e = s.db.get_endpoint(id)?.ok_or_else(|| ApiError::not_found("Endpoint"))?;
    let ou = match inp.ou_id {
        Some(o) => {
            if let Some(o) = o {
                s.db.get_ou(o)?.ok_or_else(|| ApiError::bad_request("That OU does not exist"))?;
            }
            o
        }
        None => e.ou_id,
    };
    s.db.update_endpoint_admin(id, ou, inp.notes.as_deref().unwrap_or(&e.notes))?;
    s.db.audit(admin.name(), "endpoint.update", &e.hostname, "");
    Ok(Json(s.db.get_endpoint(id)?.expect("exists")))
}

pub async fn remove(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let e = s.db.get_endpoint(id)?.ok_or_else(|| ApiError::not_found("Endpoint"))?;
    s.db.delete_endpoint(id)?;
    s.db.audit(admin.name(), "endpoint.retire", &e.hostname, "Agent credentials revoked");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- jobs

#[derive(Deserialize)]
pub struct JobsQuery {
    limit: Option<u32>,
}

/// Hide secret script parameters (passwords, tokens) from API responses.
pub fn redact(mut j: Job) -> Job {
    if let JobKind::Script { env, .. } = &mut j.kind {
        for (k, v) in env.iter_mut() {
            if od_core::scripts::is_secret_env(k) {
                *v = "••••••••".into();
            }
        }
    }
    j
}

pub async fn list_endpoint_jobs(
    State(s): State<AppState>,
    op: Operator,
    Path(id): Path<Uuid>,
    Query(q): Query<JobsQuery>,
) -> ApiResult<Json<Vec<Job>>> {
    visible(&s, &op, id)?;
    Ok(Json(s.db.list_jobs(Some(id), q.limit.unwrap_or(50).min(500))?.into_iter().map(redact).collect()))
}

pub async fn list_jobs(State(s): State<AppState>, op: Operator, Query(q): Query<JobsQuery>) -> ApiResult<Json<Vec<Job>>> {
    let mut jobs = s.db.list_jobs(None, q.limit.unwrap_or(100).min(1000))?;
    if !op.access.domain_admin {
        let eps: std::collections::HashSet<Uuid> =
            s.db.list_endpoints()?.into_iter().filter(|e| op.access.sees_endpoint(e)).map(|e| e.id).collect();
        jobs.retain(|j| eps.contains(&j.endpoint_id));
    }
    Ok(Json(jobs.into_iter().map(redact).collect()))
}

/// A job on an endpoint the operator can see, or 404.
fn visible_job(s: &AppState, op: &Operator, id: Uuid) -> ApiResult<Job> {
    let j = s.db.get_job(id)?.ok_or_else(|| ApiError::not_found("Job"))?;
    if !op.access.domain_admin {
        visible(s, op, j.endpoint_id).map_err(|_| ApiError::not_found("Job"))?;
    }
    Ok(j)
}

pub async fn get_job(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<Job>> {
    Ok(Json(redact(visible_job(&s, &op, id)?)))
}

/// The permission a delegated operator needs to queue this kind of job.
pub(crate) fn job_permission(kind: &JobKind) -> Permission {
    match kind {
        JobKind::Script { .. } => Permission::RunScripts,
        _ => Permission::EndpointActions,
    }
}

pub async fn create_job(
    State(s): State<AppState>,
    admin: Operator,
    Path(id): Path<Uuid>,
    Json(kind): Json<JobKind>,
) -> ApiResult<(StatusCode, Json<Job>)> {
    let e = visible(&s, &admin, id)?;
    admin.need_endpoint(&e, job_permission(&kind))?;
    if matches!(kind, JobKind::Unknown) {
        return Err(ApiError::bad_request("Unknown job type"));
    }
    if let JobKind::Script { script, .. } = &kind {
        if script.trim().is_empty() {
            return Err(ApiError::bad_request("The script is empty"));
        }
        if script.len() > 256 * 1024 {
            return Err(ApiError::bad_request("Scripts are limited to 256 KB"));
        }
    }
    if let JobKind::NotifyUser { title, message } = &kind {
        if title.trim().is_empty() || message.trim().is_empty() || title.len() > 120 || message.len() > 2000 {
            return Err(ApiError::bad_request("A message needs a title (up to 120 characters) and text (up to 2000)"));
        }
    }
    if matches!(kind, JobKind::RefreshInventory) {
        s.db.request_inventory(id)?;
    }
    let job = Job {
        id: Uuid::new_v4(),
        endpoint_id: id,
        kind,
        status: JobStatus::Queued,
        script_id: None,
        batch_id: None,
        exit_code: None,
        output: String::new(),
        created_by: admin.name().to_string(),
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
    };
    s.db.insert_job(&job)?;
    s.hub.wake(job.endpoint_id);
    s.db.audit(admin.name(), "job.create", &e.hostname, &job.kind.summary());
    Ok((StatusCode::CREATED, Json(job)))
}

pub async fn cancel_job(State(s): State<AppState>, admin: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<Job>> {
    let j = visible_job(&s, &admin, id)?;
    if !admin.access.domain_admin {
        let e = visible(&s, &admin, j.endpoint_id)?;
        admin.need_endpoint(&e, job_permission(&j.kind))?;
    }
    if !s.db.cancel_job(id)? {
        return Err(ApiError::conflict("Only queued jobs can be cancelled"));
    }
    s.db.audit(admin.name(), "job.cancel", &id.to_string(), "");
    Ok(Json(s.db.get_job(id)?.expect("exists")))
}

// ---------------------------------------------------------------- enrollment

#[derive(Deserialize)]
pub struct TokenInput {
    #[serde(default)]
    description: String,
    ou_id: Option<Uuid>,
    max_uses: Option<u32>,
    /// Hours until the token expires. Default 7 days; 0 means never.
    expires_in_hours: Option<i64>,
}

pub async fn list_tokens(State(s): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<EnrollmentToken>>> {
    Ok(Json(s.db.list_tokens()?))
}

pub async fn create_token(
    State(s): State<AppState>,
    admin: Admin,
    Json(inp): Json<TokenInput>,
) -> ApiResult<(StatusCode, Json<EnrollmentToken>)> {
    if let Some(o) = inp.ou_id {
        s.db.get_ou(o)?.ok_or_else(|| ApiError::bad_request("That OU does not exist"))?;
    }
    let hours = inp.expires_in_hours.unwrap_or(24 * 7);
    let t = EnrollmentToken {
        token: enrollment_code(),
        description: inp.description.trim().to_string(),
        ou_id: inp.ou_id,
        uses: 0,
        max_uses: inp.max_uses.filter(|m| *m > 0),
        expires_at: if hours > 0 { Some(Utc::now() + Duration::hours(hours)) } else { None },
        created_at: Utc::now(),
        created_by: admin.name().to_string(),
    };
    s.db.insert_token(&t)?;
    s.db.audit(admin.name(), "enrollment_token.create", &t.description, "");
    Ok((StatusCode::CREATED, Json(t)))
}

pub async fn delete_token(State(s): State<AppState>, admin: Admin, Path(token): Path<String>) -> ApiResult<StatusCode> {
    if !s.db.delete_token(&token)? {
        return Err(ApiError::not_found("Enrollment token"));
    }
    s.db.audit(admin.name(), "enrollment_token.revoke", &token[..4.min(token.len())], "");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Serialize)]
pub struct LapsView {
    account: String,
    password: String,
    previous_password: Option<String>,
    rotated_at: chrono::DateTime<Utc>,
    rotation_pending: bool,
}

/// Read the endpoint's LAPS password. Every read is audited.
pub async fn laps(State(s): State<AppState>, admin: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<LapsView>> {
    let e = visible(&s, &admin, id)?;
    admin.need_endpoint(&e, Permission::ReadLaps)?;
    let (account, cur, prev, rotated_at, pending) = s
        .db
        .get_laps(id)?
        .ok_or_else(|| ApiError::not_found("LAPS password (no GPO with the LAPS setting applies yet, or the agent hasn't rotated it)"))?;
    let open = |b: &[u8]| -> ApiResult<String> { Ok(String::from_utf8(s.sealer.open(b)?).unwrap_or_default()) };
    let view = LapsView {
        account,
        password: open(&cur)?,
        previous_password: prev.as_deref().map(open).transpose()?,
        rotated_at,
        rotation_pending: pending,
    };
    s.db.audit(admin.name(), "laps.read", &e.hostname, &format!("Viewed the password for {}", view.account));
    Ok(Json(view))
}

pub async fn laps_rotate(State(s): State<AppState>, admin: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<serde_json::Value>> {
    let e = visible(&s, &admin, id)?;
    admin.need_endpoint(&e, Permission::ReadLaps)?;
    if !s.db.request_laps_rotation(id, true)? {
        return Err(ApiError::not_found("LAPS password"));
    }
    s.hub.wake(id);
    s.db.audit(admin.name(), "laps.rotate_requested", &e.hostname, "");
    Ok(Json(serde_json::json!({ "ok": true, "message": "The endpoint rotates the password at its next check-in." })))
}

#[derive(Deserialize)]
pub struct PatchInput {
    #[serde(default)]
    targets: od_core::scripts::RunTargets,
    #[serde(default)]
    install: bool,
    #[serde(default)]
    security_only: bool,
    #[serde(default)]
    reboot: bool,
}

/// Scan for, or install, OS updates on many endpoints at once.
pub async fn patch_run(State(s): State<AppState>, admin: Admin, Json(inp): Json<PatchInput>) -> ApiResult<(StatusCode, Json<Vec<Job>>)> {
    if inp.targets.is_empty() {
        return Err(ApiError::bad_request("Choose at least one endpoint, group or OU"));
    }
    let eps = super::scripts::resolve_targets(&s, &inp.targets)?;
    let kind =
        if inp.install { JobKind::InstallUpdates { security_only: inp.security_only, reboot: inp.reboot } } else { JobKind::ScanUpdates };
    let batch = Uuid::new_v4();
    let mut jobs = vec![];
    for e in &eps {
        let job = Job {
            id: Uuid::new_v4(),
            endpoint_id: e.id,
            kind: kind.clone(),
            status: JobStatus::Queued,
            script_id: None,
            batch_id: Some(batch),
            exit_code: None,
            output: String::new(),
            created_by: admin.name().to_string(),
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
        };
        s.db.insert_job(&job)?;
        s.hub.wake(job.endpoint_id);
        jobs.push(job);
    }
    s.db.audit(
        admin.name(),
        if inp.install { "patch.install" } else { "patch.scan" },
        &format!("{} endpoint(s)", eps.len()),
        &kind.summary(),
    );
    Ok((StatusCode::CREATED, Json(jobs)))
}
