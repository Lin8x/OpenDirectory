//! Delegation of control: Domain Admins grant scoped permissions to users or
//! groups. See `crate::rbac`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::Admin;
use crate::error::{ApiError, ApiResult};
use crate::rbac::{self, Delegation, Permission, ScopeItem, ScopeKind, TrusteeKind};
use crate::AppState;

pub async fn list(State(s): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<Delegation>>> {
    Ok(Json(rbac::load(&s)))
}

#[derive(Deserialize)]
pub struct DelegationInput {
    name: String,
    #[serde(default)]
    description: String,
    trustee_kind: TrusteeKind,
    trustee_id: Uuid,
    scope: Vec<ScopeItem>,
    permissions: Vec<Permission>,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
    #[serde(default = "yes")]
    enabled: bool,
}
fn yes() -> bool {
    true
}

fn trustee_name(s: &AppState, kind: TrusteeKind, id: Uuid) -> ApiResult<String> {
    Ok(match kind {
        TrusteeKind::User => {
            let u = s.db.get_user(id)?.ok_or_else(|| ApiError::bad_request("That user does not exist"))?;
            if u.role == od_core::Role::Admin {
                return Err(ApiError::bad_request("Domain Admins already have full control; delegate to another account"));
            }
            u.username
        }
        TrusteeKind::Group => {
            let g = s.db.get_group(id)?.ok_or_else(|| ApiError::bad_request("That group does not exist"))?;
            if g.name == "Domain Admins" {
                return Err(ApiError::bad_request("Domain Admins already have full control"));
            }
            g.name
        }
    })
}

fn validate(s: &AppState, inp: &DelegationInput) -> ApiResult<String> {
    if inp.name.trim().is_empty() || inp.name.len() > 120 {
        return Err(ApiError::bad_request("Give the delegation a name (up to 120 characters)"));
    }
    if inp.permissions.is_empty() {
        return Err(ApiError::bad_request("Choose at least one permission"));
    }
    if inp.scope.is_empty() || inp.scope.len() > 500 {
        return Err(ApiError::bad_request("Choose at least one endpoint, OU or group to delegate"));
    }
    for item in &inp.scope {
        let found = match item.kind {
            ScopeKind::Endpoint => s.db.get_endpoint(item.id)?.is_some(),
            ScopeKind::Ou => s.db.get_ou(item.id)?.is_some(),
            ScopeKind::Group => s.db.get_group(item.id)?.is_some(),
        };
        if !found {
            return Err(ApiError::bad_request("A chosen endpoint, OU or group no longer exists"));
        }
    }
    if inp.expires_at.is_some_and(|t| t <= Utc::now()) {
        return Err(ApiError::bad_request("The expiry time is in the past"));
    }
    trustee_name(s, inp.trustee_kind, inp.trustee_id)
}

fn describe(d: &Delegation, who: &str) -> String {
    let perms: Vec<String> =
        d.permissions.iter().map(|p| serde_json::to_value(p).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()).collect();
    let exp = d.expires_at.map(|t| format!(", expires {}", t.format("%Y-%m-%d %H:%M UTC"))).unwrap_or_default();
    format!("To {who}: {} on {} object(s){exp}", perms.join(", "), d.scope.len())
}

pub async fn create(
    State(s): State<AppState>,
    admin: Admin,
    Json(inp): Json<DelegationInput>,
) -> ApiResult<(StatusCode, Json<Delegation>)> {
    let who = validate(&s, &inp)?;
    let d = Delegation {
        id: Uuid::new_v4(),
        name: inp.name.trim().to_string(),
        description: inp.description.trim().to_string(),
        trustee_kind: inp.trustee_kind,
        trustee_id: inp.trustee_id,
        scope: inp.scope,
        permissions: inp.permissions,
        expires_at: inp.expires_at,
        enabled: inp.enabled,
        created_by: admin.name().to_string(),
        created_at: Utc::now(),
    };
    let mut all = rbac::load(&s);
    all.push(d.clone());
    rbac::save(&s, &all)?;
    s.db.audit(admin.name(), "delegation.create", &d.name, &describe(&d, &who));
    Ok((StatusCode::CREATED, Json(d)))
}

pub async fn update(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<DelegationInput>,
) -> ApiResult<Json<Delegation>> {
    let mut all = rbac::load(&s);
    let i = all.iter().position(|d| d.id == id).ok_or_else(|| ApiError::not_found("Delegation"))?;
    // Allow saving an already-expired delegation only to switch it off.
    let who = if inp.expires_at.is_some_and(|t| t <= Utc::now()) && !inp.enabled {
        trustee_name(&s, inp.trustee_kind, inp.trustee_id)?
    } else {
        validate(&s, &inp)?
    };
    let d = &mut all[i];
    d.name = inp.name.trim().to_string();
    d.description = inp.description.trim().to_string();
    d.trustee_kind = inp.trustee_kind;
    d.trustee_id = inp.trustee_id;
    d.scope = inp.scope;
    d.permissions = inp.permissions;
    d.expires_at = inp.expires_at;
    d.enabled = inp.enabled;
    let d = d.clone();
    rbac::save(&s, &all)?;
    s.db.audit(admin.name(), "delegation.update", &d.name, &describe(&d, &who));
    Ok(Json(d))
}

pub async fn remove(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let mut all = rbac::load(&s);
    let i = all.iter().position(|d| d.id == id).ok_or_else(|| ApiError::not_found("Delegation"))?;
    let d = all.remove(i);
    rbac::save(&s, &all)?;
    s.db.audit(admin.name(), "delegation.delete", &d.name, "");
    Ok(StatusCode::NO_CONTENT)
}
