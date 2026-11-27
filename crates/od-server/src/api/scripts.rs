//! Script Library: saved scripts, built-in templates, and runs against
//! endpoints, security groups and OUs.

use std::collections::{BTreeMap, HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use od_core::policy::Setting;
use od_core::scripts::{param_env, templates, valid_param_name, RunTargets, Script, ScriptParam, ScriptTemplate};
use od_core::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::required;
use crate::auth::{Admin, Operator};
use crate::error::{ApiError, ApiResult};
use crate::rbac::Permission;
use crate::AppState;

pub async fn list_templates(_admin: Admin) -> Json<Vec<ScriptTemplate>> {
    Json(templates())
}

pub async fn list(State(s): State<AppState>, op: Operator) -> ApiResult<Json<Vec<Script>>> {
    if !op.access.has_any(Permission::RunScripts) {
        return Err(ApiError::forbidden("Your delegation doesn't include running scripts"));
    }
    Ok(Json(s.db.list_scripts()?))
}

pub async fn get(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<Script>> {
    if !op.access.has_any(Permission::RunScripts) {
        return Err(ApiError::not_found("Script"));
    }
    Ok(Json(s.db.get_script(id)?.ok_or_else(|| ApiError::not_found("Script"))?))
}

#[derive(Deserialize)]
pub struct ScriptInput {
    /// Copy a built-in template; the other fields override it.
    template: Option<String>,
    name: Option<String>,
    description: Option<String>,
    category: Option<String>,
    shell: Option<Shell>,
    body: Option<String>,
    os: Option<Vec<OsFamily>>,
    params: Option<Vec<ScriptParam>>,
}

fn validate(sc: &Script) -> ApiResult<()> {
    if sc.body.trim().is_empty() {
        return Err(ApiError::bad_request("The script is empty"));
    }
    if sc.body.len() > 256 * 1024 {
        return Err(ApiError::bad_request("Scripts are limited to 256 KB"));
    }
    if sc.name.len() > 120 {
        return Err(ApiError::bad_request("The name is too long"));
    }
    let mut seen = HashSet::new();
    for p in &sc.params {
        if !valid_param_name(&p.name) {
            return Err(ApiError::bad_request(format!(
                "Parameter \"{}\" must use capital letters, digits and underscores, and start with a letter",
                p.name
            )));
        }
        if !seen.insert(&p.name) {
            return Err(ApiError::bad_request(format!("Parameter \"{}\" appears twice", p.name)));
        }
    }
    if sc.params.len() > 20 {
        return Err(ApiError::bad_request("Scripts can have at most 20 parameters"));
    }
    Ok(())
}

pub async fn create(State(s): State<AppState>, admin: Admin, Json(inp): Json<ScriptInput>) -> ApiResult<(StatusCode, Json<Script>)> {
    let tpl = match &inp.template {
        Some(k) => Some(templates().into_iter().find(|t| t.key == k).ok_or_else(|| ApiError::bad_request("Unknown template"))?),
        None => None,
    };
    let now = Utc::now();
    let sc = Script {
        id: Uuid::new_v4(),
        name: required("Name", inp.name.as_deref().or(tpl.as_ref().map(|t| t.name)).unwrap_or(""))?,
        description: inp.description.or(tpl.as_ref().map(|t| t.description.to_string())).unwrap_or_default(),
        category: inp.category.or(tpl.as_ref().map(|t| t.category.to_string())).unwrap_or_else(|| "General".into()),
        shell: inp.shell.or(tpl.as_ref().map(|t| t.shell.clone())).unwrap_or(Shell::Default),
        body: inp.body.or(tpl.as_ref().map(|t| t.body.to_string())).unwrap_or_default(),
        os: inp.os.or(tpl.as_ref().map(|t| t.os.clone())).unwrap_or_default(),
        params: inp.params.or(tpl.as_ref().map(|t| t.params.clone())).unwrap_or_default(),
        template: tpl.as_ref().map(|t| t.key.to_string()),
        created_by: admin.name().to_string(),
        created_at: now,
        updated_at: now,
    };
    validate(&sc)?;
    s.db.upsert_script(&sc)?;
    s.db.audit(
        admin.name(),
        "script.create",
        &sc.name,
        sc.template.as_deref().map(|t| format!("from template {t}")).as_deref().unwrap_or(""),
    );
    Ok((StatusCode::CREATED, Json(sc)))
}

pub async fn update(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<ScriptInput>,
) -> ApiResult<Json<Script>> {
    let mut sc = s.db.get_script(id)?.ok_or_else(|| ApiError::not_found("Script"))?;
    if let Some(n) = inp.name {
        sc.name = required("Name", &n)?;
    }
    if let Some(v) = inp.description {
        sc.description = v;
    }
    if let Some(v) = inp.category {
        sc.category = v;
    }
    if let Some(v) = inp.shell {
        sc.shell = v;
    }
    if let Some(v) = inp.body {
        sc.body = v;
    }
    if let Some(v) = inp.os {
        sc.os = v;
    }
    if let Some(v) = inp.params {
        sc.params = v;
    }
    sc.updated_at = Utc::now();
    validate(&sc)?;
    s.db.upsert_script(&sc)?;
    s.db.audit(admin.name(), "script.update", &sc.name, "");
    Ok(Json(sc))
}

/// GPOs that run this script.
fn used_by(s: &AppState, id: Uuid) -> ApiResult<Vec<String>> {
    Ok(s.db
        .list_policies()?
        .into_iter()
        .filter(|p| p.settings.iter().any(|st| matches!(st, Setting::RunScript { script_id, .. } if *script_id == id)))
        .map(|p| p.name)
        .collect())
}

pub async fn remove(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let sc = s.db.get_script(id)?.ok_or_else(|| ApiError::not_found("Script"))?;
    let users = used_by(&s, id)?;
    if !users.is_empty() {
        return Err(ApiError::conflict(format!("This script is used by {}. Remove it from those GPOs first.", users.join(", "))));
    }
    super::recycle::keep(&s, super::recycle::Snapshot::Script { script: sc.clone() }, admin.name())?;
    s.db.delete_script(id)?;
    s.db.audit(admin.name(), "script.delete", &sc.name, "Moved to the Recycle Bin");
    Ok(StatusCode::NO_CONTENT)
}

/// Every endpoint a set of targets reaches: explicit endpoints, computer
/// members of groups, and endpoints in OUs (including child OUs).
pub fn resolve_targets(s: &AppState, t: &RunTargets) -> ApiResult<Vec<Endpoint>> {
    let all = s.db.list_endpoints()?;
    if t.all_endpoints {
        return Ok(all);
    }
    let ou_parent: HashMap<Uuid, Option<Uuid>> = s.db.list_ous()?.into_iter().map(|o| (o.id, o.parent_id)).collect();
    let mut ids: HashSet<Uuid> = t.endpoint_ids.iter().copied().collect();
    for g in &t.group_ids {
        let g = s.db.get_group(*g)?.ok_or_else(|| ApiError::bad_request("A target group no longer exists"))?;
        ids.extend(g.endpoint_ids);
    }
    for o in &t.ou_ids {
        if !ou_parent.contains_key(o) {
            return Err(ApiError::bad_request("A target OU no longer exists"));
        }
    }
    let in_ou = |mut cur: Option<Uuid>| {
        let mut guard = 0;
        while let Some(ou) = cur {
            if t.ou_ids.contains(&ou) {
                return true;
            }
            cur = ou_parent.get(&ou).copied().flatten();
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        false
    };
    Ok(all.into_iter().filter(|e| ids.contains(&e.id) || in_ou(e.ou_id)).collect())
}

#[derive(Deserialize)]
pub struct RunInput {
    #[serde(default)]
    targets: RunTargets,
    #[serde(default)]
    params: BTreeMap<String, String>,
    /// Only report which endpoints would run it.
    #[serde(default)]
    dry_run: bool,
}

#[derive(Serialize)]
pub struct TargetPreview {
    id: Uuid,
    hostname: String,
    os: OsFamily,
    online: bool,
}

#[derive(Serialize)]
pub struct RunResponse {
    batch_id: Option<Uuid>,
    /// Endpoints that get the script.
    targets: Vec<TargetPreview>,
    /// Endpoints reached by the targets but skipped because the script isn't for their OS.
    skipped: Vec<TargetPreview>,
    jobs: Vec<Job>,
}

fn preview(e: &Endpoint) -> TargetPreview {
    TargetPreview { id: e.id, hostname: e.hostname.clone(), os: e.os, online: e.online }
}

pub async fn run(
    State(s): State<AppState>,
    admin: Operator,
    Path(id): Path<Uuid>,
    Json(inp): Json<RunInput>,
) -> ApiResult<(StatusCode, Json<RunResponse>)> {
    if !admin.access.has_any(Permission::RunScripts) {
        return Err(ApiError::forbidden("Your delegation doesn't include running scripts"));
    }
    let sc = s.db.get_script(id)?.ok_or_else(|| ApiError::not_found("Script"))?;
    if inp.targets.is_empty() {
        return Err(ApiError::bad_request("Choose at least one endpoint, group or OU"));
    }
    let env = param_env(&sc.params, &inp.params).map_err(ApiError::bad_request)?;
    let resolved = resolve_targets(&s, &inp.targets)?;
    // A delegated operator may only target endpoints they can run scripts on;
    // an OU or group target that reaches further is refused outright.
    if resolved.iter().any(|e| !admin.access.endpoint(e, Permission::RunScripts)) {
        return Err(ApiError::forbidden("Some of the chosen targets are outside your delegation"));
    }
    let (targets, skipped): (Vec<Endpoint>, Vec<Endpoint>) = resolved.into_iter().partition(|e| sc.supports(e.os));
    if inp.dry_run {
        return Ok((
            StatusCode::OK,
            Json(RunResponse {
                batch_id: None,
                targets: targets.iter().map(preview).collect(),
                skipped: skipped.iter().map(preview).collect(),
                jobs: vec![],
            }),
        ));
    }
    if targets.is_empty() {
        return Err(ApiError::bad_request("None of the chosen targets run an OS this script supports"));
    }
    let batch = Uuid::new_v4();
    let now = Utc::now();
    let mut jobs = Vec::new();
    for e in &targets {
        let job = Job {
            id: Uuid::new_v4(),
            endpoint_id: e.id,
            kind: JobKind::Script { shell: sc.shell.clone(), script: sc.body.clone(), env: env.clone() },
            status: JobStatus::Queued,
            script_id: Some(sc.id),
            batch_id: Some(batch),
            exit_code: None,
            output: String::new(),
            created_by: admin.name().to_string(),
            created_at: now,
            started_at: None,
            finished_at: None,
        };
        s.db.insert_job(&job)?;
        s.hub.wake(job.endpoint_id);
        jobs.push(job);
    }
    s.db.audit(admin.name(), "script.run", &sc.name, &format!("{} endpoint(s)", targets.len()));
    Ok((
        StatusCode::CREATED,
        Json(RunResponse {
            batch_id: Some(batch),
            targets: targets.iter().map(preview).collect(),
            skipped: skipped.iter().map(preview).collect(),
            jobs,
        }),
    ))
}

#[derive(Deserialize)]
pub struct LimitQuery {
    limit: Option<u32>,
}

pub async fn script_jobs(
    State(s): State<AppState>,
    _admin: Admin,
    Path(id): Path<Uuid>,
    Query(q): Query<LimitQuery>,
) -> ApiResult<Json<Vec<Job>>> {
    Ok(Json(s.db.list_script_jobs(id, q.limit.unwrap_or(100).min(1000))?.into_iter().map(super::endpoints::redact).collect()))
}

pub async fn batch_jobs(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<Job>>> {
    let mut jobs = s.db.list_batch_jobs(id)?;
    if !op.access.domain_admin {
        let eps: std::collections::HashSet<Uuid> =
            s.db.list_endpoints()?.into_iter().filter(|e| op.access.sees_endpoint(e)).map(|e| e.id).collect();
        jobs.retain(|j| eps.contains(&j.endpoint_id));
    }
    Ok(Json(jobs.into_iter().map(super::endpoints::redact).collect()))
}

#[derive(Serialize)]
pub struct ScriptUsage {
    gpos: Vec<String>,
}

pub async fn usage(State(s): State<AppState>, _admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<ScriptUsage>> {
    Ok(Json(ScriptUsage { gpos: used_by(&s, id)? }))
}
