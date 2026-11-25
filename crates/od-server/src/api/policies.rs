//! Group Policy Objects (cross-platform) and Resultant Set of Policy.

use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use od_core::policy::{catalog as policy_catalog, effective_settings, CatalogEntry, Policy, PolicyTargets, Setting};
use od_core::ComplianceItem;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::required;
use crate::auth::Admin;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub async fn catalog(_admin: Admin) -> Json<Vec<CatalogEntry>> {
    Json(policy_catalog())
}

pub async fn list(State(s): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<Policy>>> {
    Ok(Json(s.db.list_policies()?))
}

pub async fn get(State(s): State<AppState>, _admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<Policy>> {
    Ok(Json(s.db.get_policy(id)?.ok_or_else(|| ApiError::not_found("Policy"))?))
}

#[derive(Deserialize)]
pub struct PolicyInput {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default = "default_priority")]
    priority: i32,
    #[serde(default)]
    targets: PolicyTargets,
    #[serde(default)]
    settings: Vec<Setting>,
}
fn yes() -> bool {
    true
}
fn default_priority() -> i32 {
    100
}

fn validate(s: &AppState, inp: &PolicyInput) -> ApiResult<()> {
    let mut kinds = std::collections::HashSet::new();
    for st in &inp.settings {
        if !st.accumulates() && !kinds.insert(st.kind()) {
            return Err(ApiError::bad_request(format!("\"{}\" appears twice in this policy", st.kind())));
        }
        match st {
            Setting::PasswordMinLength { length } if !(4..=128).contains(length) => {
                return Err(ApiError::bad_request("Minimum password length must be between 4 and 128"))
            }
            Setting::ScreenLock { minutes } if !(1..=240).contains(minutes) => {
                return Err(ApiError::bad_request("Screen lock must be between 1 and 240 minutes"))
            }
            Setting::LocalAccounts { group_id, .. } => {
                s.db.get_group(*group_id)?.ok_or_else(|| ApiError::bad_request("Choose a security group for sign-in accounts"))?;
            }
            Setting::InstallPackages { packages } => {
                for p in packages {
                    let ok = !p.is_empty() && p.len() <= 128 && p.chars().all(|c| c.is_ascii_alphanumeric() || "._-+@/".contains(c));
                    if !ok {
                        return Err(ApiError::bad_request(format!("\"{p}\" is not a valid package name")));
                    }
                }
            }
            Setting::LoginBanner { title, text } if title.len() > 200 || text.len() > 4000 => {
                return Err(ApiError::bad_request("The login notice is too long"))
            }
            _ => {}
        }
    }
    for ou in &inp.targets.ou_ids {
        s.db.get_ou(*ou)?.ok_or_else(|| ApiError::bad_request("A linked OU no longer exists"))?;
    }
    Ok(())
}

pub async fn create(State(s): State<AppState>, admin: Admin, Json(inp): Json<PolicyInput>) -> ApiResult<(StatusCode, Json<Policy>)> {
    let name = required("Name", &inp.name)?;
    validate(&s, &inp)?;
    let now = Utc::now();
    let p = Policy {
        id: Uuid::new_v4(),
        name,
        description: inp.description,
        enabled: inp.enabled,
        priority: inp.priority,
        targets: inp.targets,
        settings: inp.settings,
        created_at: now,
        updated_at: now,
    };
    s.db.upsert_policy(&p)?;
    s.db.audit(admin.name(), "policy.create", &p.name, &format!("{} settings", p.settings.len()));
    Ok((StatusCode::CREATED, Json(p)))
}

pub async fn update(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>, Json(inp): Json<PolicyInput>) -> ApiResult<Json<Policy>> {
    let existing = s.db.get_policy(id)?.ok_or_else(|| ApiError::not_found("Policy"))?;
    let name = required("Name", &inp.name)?;
    validate(&s, &inp)?;
    let p = Policy {
        id,
        name,
        description: inp.description,
        enabled: inp.enabled,
        priority: inp.priority,
        targets: inp.targets,
        settings: inp.settings,
        created_at: existing.created_at,
        updated_at: Utc::now(),
    };
    s.db.upsert_policy(&p)?;
    s.db.audit(admin.name(), "policy.update", &p.name, if p.enabled { "" } else { "disabled" });
    Ok(Json(p))
}

pub async fn remove(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let p = s.db.get_policy(id)?.ok_or_else(|| ApiError::not_found("Policy"))?;
    s.db.delete_policy(id)?;
    s.db.audit(admin.name(), "policy.delete", &p.name, "");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct RsopEntry {
    #[serde(flatten)]
    effective: od_core::policy::EffectiveSetting,
    description: String,
    support: Option<od_core::policy::PlatformSupport>,
    compliance: Option<ComplianceItem>,
}

/// Resultant Set of Policy for one endpoint: every setting that applies,
/// which GPO it came from, what it lost to, and its last reported state.
pub async fn rsop(State(s): State<AppState>, _admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<RsopEntry>>> {
    let e = s.db.get_endpoint(id)?.ok_or_else(|| ApiError::not_found("Endpoint"))?;
    let ou_parent: HashMap<Uuid, Option<Uuid>> = s.db.list_ous()?.into_iter().map(|o| (o.id, o.parent_id)).collect();
    let eff = effective_settings(&s.db.list_policies()?, e.ou_id, &ou_parent);
    let cat = policy_catalog();
    Ok(Json(
        eff.into_iter()
            .map(|es| {
                let kind = es.setting.kind();
                let support = cat.iter().find(|c| c.kind == kind).and_then(|c| c.support.iter().find(|p| p.os == e.os).cloned());
                let compliance = e.compliance.iter().find(|c| c.setting == kind).cloned();
                RsopEntry { description: es.setting.describe(), effective: es, support, compliance }
            })
            .collect(),
    ))
}
