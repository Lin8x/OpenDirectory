//! Endpoint management: services, installed software, processes and
//! containers on one endpoint, like Computer Management and Windows Admin
//! Center, plus a small built-in Portainer.
//!
//! Reads are *queries*, relayed live to the agent over its wake-up long poll
//! and never stored. Changes are *actions*, queued as jobs (so they're in the
//! job history with their output) and audit-logged. Everything checks the
//! operator's delegated permissions and the Management tools GPO setting.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use od_core::manage::{Area, ContainerExec, ManageAction, ManageQuery, QueryRequest, QueryResult};
use od_core::policy::{version_at_least, Setting};
use od_core::{Endpoint, Job, JobKind, JobStatus};
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Notify};
use uuid::Uuid;

use super::endpoints::visible;
use crate::auth::{EndpointCaller, Operator};
use crate::error::{ApiError, ApiResult};
use crate::rbac::Permission;
use crate::AppState;

const QUERY_TIMEOUT: Duration = Duration::from_secs(45);
const PORTAINER_KEY: &str = "portainer_url";

// ------------------------------------------------------------- query relay

struct Waiting {
    endpoint: Uuid,
    tx: oneshot::Sender<QueryResult>,
}

#[derive(Default)]
struct Relay {
    pending: Mutex<HashMap<Uuid, Vec<QueryRequest>>>,
    waiting: Mutex<HashMap<Uuid, Waiting>>,
    wakers: Mutex<HashMap<Uuid, Arc<Notify>>>,
}

static RELAY: LazyLock<Relay> = LazyLock::new(Relay::default);

/// Wakes the endpoint's long poll when a query is waiting for it.
pub(crate) fn waker(endpoint: Uuid) -> Arc<Notify> {
    RELAY.wakers.lock().unwrap_or_else(|e| e.into_inner()).entry(endpoint).or_default().clone()
}

/// Queries for this endpoint's agent, handed out once.
pub(crate) fn take_queries(endpoint: Uuid) -> Vec<QueryRequest> {
    RELAY.pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&endpoint).unwrap_or_default()
}

/// Removes a query that timed out, whether or not it was delivered.
struct Cleanup(Uuid, Uuid);
impl Drop for Cleanup {
    fn drop(&mut self) {
        RELAY.waiting.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.1);
        if let Some(v) = RELAY.pending.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&self.0) {
            v.retain(|q| q.id != self.1);
        }
    }
}

async fn ask(e: &Endpoint, query: ManageQuery) -> ApiResult<serde_json::Value> {
    let id = Uuid::new_v4();
    let (tx, rx) = oneshot::channel();
    RELAY.waiting.lock().unwrap_or_else(|e| e.into_inner()).insert(id, Waiting { endpoint: e.id, tx });
    RELAY.pending.lock().unwrap_or_else(|e| e.into_inner()).entry(e.id).or_default().push(QueryRequest { id, query });
    let _cleanup = Cleanup(e.id, id);
    waker(e.id).notify_one();
    match tokio::time::timeout(QUERY_TIMEOUT, rx).await {
        Ok(Ok(r)) if r.ok => Ok(r.data),
        Ok(Ok(r)) => {
            Err(ApiError::new(StatusCode::BAD_GATEWAY, if r.error.is_empty() { "The endpoint couldn't answer".into() } else { r.error }))
        }
        _ => Err(ApiError::new(
            StatusCode::GATEWAY_TIMEOUT,
            format!(
                "{} didn't answer within {} seconds. Check that it's online and its agent is running.",
                e.hostname,
                QUERY_TIMEOUT.as_secs()
            ),
        )),
    }
}

/// The agent's answer to a query.
pub async fn agent_result(EndpointCaller(auth): EndpointCaller, Path(id): Path<Uuid>, Json(r): Json<QueryResult>) -> ApiResult<StatusCode> {
    let w = {
        let mut m = RELAY.waiting.lock().unwrap_or_else(|e| e.into_inner());
        match m.get(&id) {
            Some(w) if w.endpoint == auth.id => m.remove(&id),
            _ => None,
        }
    };
    match w {
        Some(w) => {
            let _ = w.tx.send(r);
            Ok(StatusCode::NO_CONTENT)
        }
        None => Err(ApiError::not_found("Query")),
    }
}

// ------------------------------------------------------------------ checks

pub(crate) fn area_permission(a: Area) -> Permission {
    match a {
        Area::Services => Permission::ManageServices,
        Area::Software => Permission::ManageSoftware,
        Area::Processes => Permission::ManageProcesses,
        Area::Containers => Permission::ManageContainers,
    }
}

/// Is the area turned on for this endpoint by the Management tools setting?
fn gpo_allows(s: &AppState, e: &Endpoint, a: Area) -> ApiResult<()> {
    for st in super::policies::endpoint_settings(s, e)? {
        if let Setting::ManagementTools { services, software, processes, containers } = st {
            let on = match a {
                Area::Services => services,
                Area::Software => software,
                Area::Processes => processes,
                Area::Containers => containers,
            };
            if !on {
                return Err(ApiError::forbidden(format!(
                    "{} is turned off for this endpoint by the Management tools GPO setting",
                    a.label()
                )));
            }
        }
    }
    Ok(())
}

fn supported(e: &Endpoint) -> ApiResult<()> {
    if e.os == od_core::OsFamily::Android {
        return Err(ApiError::bad_request("Android devices can't be managed this way"));
    }
    if !version_at_least(&e.agent_version, od_core::manage::MIN_AGENT) {
        return Err(ApiError::conflict(format!(
            "{} runs Endpoint Agent {}; endpoint management needs {} or later. Update the agent first.",
            e.hostname,
            if e.agent_version.is_empty() { "(unknown)" } else { &e.agent_version },
            od_core::manage::MIN_AGENT
        )));
    }
    Ok(())
}

/// For reading: the area's permission, or View endpoints.
fn can_view(s: &AppState, op: &Operator, id: Uuid, a: Area) -> ApiResult<Endpoint> {
    let e = visible(s, op, id)?;
    if !op.access.endpoint(&e, Permission::ViewEndpoints) {
        op.need_endpoint(&e, area_permission(a))?;
    }
    supported(&e)?;
    gpo_allows(s, &e, a)?;
    Ok(e)
}

/// For changing: the area's permission.
fn can_manage(s: &AppState, op: &Operator, id: Uuid, a: Area) -> ApiResult<Endpoint> {
    let e = visible(s, op, id)?;
    op.need_endpoint(&e, area_permission(a))?;
    supported(&e)?;
    gpo_allows(s, &e, a)?;
    Ok(e)
}

// ------------------------------------------------------------------- reads

pub async fn services(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<serde_json::Value>> {
    let e = can_view(&s, &op, id, Area::Services)?;
    Ok(Json(ask(&e, ManageQuery::Services).await?))
}

pub async fn software(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<serde_json::Value>> {
    let e = can_view(&s, &op, id, Area::Software)?;
    Ok(Json(ask(&e, ManageQuery::Software).await?))
}

pub async fn processes(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<serde_json::Value>> {
    let e = can_view(&s, &op, id, Area::Processes)?;
    Ok(Json(ask(&e, ManageQuery::Processes).await?))
}

pub async fn containers(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<serde_json::Value>> {
    let e = can_view(&s, &op, id, Area::Containers)?;
    Ok(Json(ask(&e, ManageQuery::Containers).await?))
}

#[derive(Deserialize)]
pub struct LogsQuery {
    #[serde(default)]
    tail: Option<u32>,
}

#[derive(Serialize)]
pub struct Logs {
    logs: serde_json::Value,
}

/// Container logs can hold secrets, so reading them needs Manage containers
/// and is audited.
pub async fn container_logs(
    State(s): State<AppState>,
    op: Operator,
    Path((id, cid)): Path<(Uuid, String)>,
    Query(q): Query<LogsQuery>,
) -> ApiResult<Json<Logs>> {
    let e = can_manage(&s, &op, id, Area::Containers)?;
    od_core::manage::check_name("container id", &cid, ".:-_/@").map_err(ApiError::bad_request)?;
    let logs = ask(&e, ManageQuery::ContainerLogs { id: cid.clone(), tail: q.tail.unwrap_or(500).clamp(10, 5000) }).await?;
    s.db.audit(op.name(), "container.logs", &e.hostname, &cid);
    Ok(Json(Logs { logs }))
}

// ----------------------------------------------------------------- actions

/// Queue a management action as a job.
pub async fn act(
    State(s): State<AppState>,
    op: Operator,
    Path(id): Path<Uuid>,
    Json(action): Json<ManageAction>,
) -> ApiResult<(StatusCode, Json<Job>)> {
    let area = action.area().ok_or_else(|| ApiError::bad_request("Unknown management action"))?;
    let e = can_manage(&s, &op, id, area)?;
    action.validate().map_err(ApiError::bad_request)?;
    let job = Job {
        id: Uuid::new_v4(),
        endpoint_id: id,
        kind: JobKind::Manage { action: action.clone() },
        status: JobStatus::Queued,
        script_id: None,
        batch_id: None,
        exit_code: None,
        output: String::new(),
        created_by: op.name().to_string(),
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
    };
    s.db.insert_job(&job)?;
    s.hub.wake(id);
    s.db.audit(op.name(), &action.audit_action(), &e.hostname, &action.summary());
    Ok((StatusCode::CREATED, Json(job)))
}

// -------------------------------------------------------------------- exec

#[derive(Deserialize)]
pub struct ExecInput {
    #[serde(default = "cols")]
    cols: u16,
    #[serde(default = "rows")]
    rows: u16,
    /// Shell inside the container (sh, bash…); empty tries bash, then sh.
    #[serde(default)]
    command: String,
}
fn cols() -> u16 {
    120
}
fn rows() -> u16 {
    32
}

/// `docker exec -it` into a container from the in-browser terminal. Needs
/// Remote shell and Manage containers, and both GPO switches on.
pub async fn exec(
    State(s): State<AppState>,
    op: Operator,
    Path((id, cid)): Path<(Uuid, String)>,
    Json(inp): Json<ExecInput>,
) -> ApiResult<Json<super::shell::Opened>> {
    let e = can_manage(&s, &op, id, Area::Containers)?;
    od_core::manage::check_name("container", &cid, ".:-_/@").map_err(ApiError::bad_request)?;
    const SHELLS: &[&str] = &["", "sh", "bash", "ash", "zsh", "/bin/sh", "/bin/bash", "powershell", "pwsh", "cmd"];
    if !SHELLS.contains(&inp.command.as_str()) {
        return Err(ApiError::bad_request("Pick sh, bash, ash, zsh, powershell, pwsh or cmd"));
    }
    let exec = ContainerExec { container: cid.clone(), command: inp.command };
    let opened = super::shell::start_exec(&s, &op, &e, inp.cols, inp.rows, exec)?;
    s.db.audit(op.name(), "container.exec", &e.hostname, &format!("Shell in container {cid}"));
    Ok(Json(opened))
}

// --------------------------------------------------------------- Portainer

#[derive(Serialize, Deserialize)]
pub struct PortainerLink {
    /// Your existing Portainer, e.g. https://portainer.corp.example:9443. Empty for none.
    #[serde(default)]
    url: String,
}

pub async fn get_portainer(State(s): State<AppState>, _op: Operator) -> ApiResult<Json<PortainerLink>> {
    Ok(Json(PortainerLink { url: s.db.meta_get(PORTAINER_KEY)?.unwrap_or_default() }))
}

pub async fn set_portainer(
    State(s): State<AppState>,
    admin: crate::auth::Admin,
    Json(inp): Json<PortainerLink>,
) -> ApiResult<Json<PortainerLink>> {
    let url = inp.url.trim().trim_end_matches('/').to_string();
    if !url.is_empty() {
        let ok = reqwest::Url::parse(&url).map(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some()).unwrap_or(false);
        if !ok {
            return Err(ApiError::bad_request("Enter the Portainer address, like https://portainer.example.com:9443"));
        }
    }
    s.db.meta_set(PORTAINER_KEY, &url)?;
    s.db.audit(&admin.user.username, "portainer.link", if url.is_empty() { "(removed)" } else { &url }, "");
    Ok(Json(PortainerLink { url }))
}
