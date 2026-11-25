//! Endpoints, enrollment tokens and remote scripts (jobs).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{Duration, Utc};
use od_core::*;
use serde::Deserialize;
use uuid::Uuid;

use super::directory::double_option;
use crate::auth::Admin;
use crate::crypto::enrollment_code;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub async fn list(State(s): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<Endpoint>>> {
    Ok(Json(s.db.list_endpoints()?))
}

pub async fn get(State(s): State<AppState>, _admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<Endpoint>> {
    Ok(Json(s.db.get_endpoint(id)?.ok_or_else(|| ApiError::not_found("Endpoint"))?))
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

pub async fn list_endpoint_jobs(
    State(s): State<AppState>,
    _admin: Admin,
    Path(id): Path<Uuid>,
    Query(q): Query<JobsQuery>,
) -> ApiResult<Json<Vec<Job>>> {
    Ok(Json(s.db.list_jobs(Some(id), q.limit.unwrap_or(50).min(500))?))
}

pub async fn list_jobs(State(s): State<AppState>, _admin: Admin, Query(q): Query<JobsQuery>) -> ApiResult<Json<Vec<Job>>> {
    Ok(Json(s.db.list_jobs(None, q.limit.unwrap_or(100).min(1000))?))
}

pub async fn get_job(State(s): State<AppState>, _admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<Job>> {
    Ok(Json(s.db.get_job(id)?.ok_or_else(|| ApiError::not_found("Job"))?))
}

pub async fn create_job(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(kind): Json<JobKind>,
) -> ApiResult<(StatusCode, Json<Job>)> {
    let e = s.db.get_endpoint(id)?.ok_or_else(|| ApiError::not_found("Endpoint"))?;
    if let JobKind::Script { script, .. } = &kind {
        if script.trim().is_empty() {
            return Err(ApiError::bad_request("The script is empty"));
        }
        if script.len() > 256 * 1024 {
            return Err(ApiError::bad_request("Scripts are limited to 256 KB"));
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
    s.db.audit(admin.name(), "job.create", &e.hostname, &job.kind.summary());
    Ok((StatusCode::CREATED, Json(job)))
}

pub async fn cancel_job(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<Job>> {
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
pub async fn laps(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<LapsView>> {
    let e = s.db.get_endpoint(id)?.ok_or_else(|| ApiError::not_found("Endpoint"))?;
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

pub async fn laps_rotate(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<serde_json::Value>> {
    let e = s.db.get_endpoint(id)?.ok_or_else(|| ApiError::not_found("Endpoint"))?;
    if !s.db.request_laps_rotation(id, true)? {
        return Err(ApiError::not_found("LAPS password"));
    }
    s.db.audit(admin.name(), "laps.rotate_requested", &e.hostname, "");
    Ok(Json(serde_json::json!({ "ok": true, "message": "The endpoint rotates the password at its next check-in." })))
}
