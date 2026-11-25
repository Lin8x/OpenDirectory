//! Recycle Bin: deleted users, groups, OUs, GPOs and scripts are kept for a
//! while (180 days by default) with all their attributes, group memberships
//! and password, so they can be restored exactly, like the AD Recycle Bin.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use base64::Engine;
use chrono::{DateTime, Utc};
use od_core::policy::Policy;
use od_core::scripts::Script;
use od_core::{Group, OrgUnit, User};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::Admin;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub const DEFAULT_RETENTION_DAYS: u32 = 180;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Snapshot {
    User { user: User, password_hash: Option<String>, password_sealed: Option<String> },
    Group { group: Group },
    Ou { ou: OrgUnit },
    Policy { policy: Policy },
    Script { script: Script },
}

impl Snapshot {
    fn kind(&self) -> &'static str {
        match self {
            Snapshot::User { .. } => "user",
            Snapshot::Group { .. } => "group",
            Snapshot::Ou { .. } => "ou",
            Snapshot::Policy { .. } => "policy",
            Snapshot::Script { .. } => "script",
        }
    }
    fn name(&self) -> String {
        match self {
            Snapshot::User { user, .. } => format!("{} ({})", user.display_name, user.username),
            Snapshot::Group { group } => group.name.clone(),
            Snapshot::Ou { ou } => ou.name.clone(),
            Snapshot::Policy { policy } => policy.name.clone(),
            Snapshot::Script { script } => script.name.clone(),
        }
    }
}

/// Keep a copy before an object is deleted.
pub fn keep(s: &AppState, snap: Snapshot, actor: &str) -> ApiResult<()> {
    let data = serde_json::to_string(&snap).map_err(anyhow::Error::from)?;
    s.db.insert_recycled(Uuid::new_v4(), snap.kind(), &snap.name(), &data, actor)?;
    Ok(())
}

pub fn keep_user(s: &AppState, u: &User, actor: &str) -> ApiResult<()> {
    let sec = s.db.user_secrets(u.id)?;
    let b64 = base64::engine::general_purpose::STANDARD;
    keep(
        s,
        Snapshot::User { user: u.clone(), password_hash: sec.password_hash, password_sealed: sec.password_sealed.map(|b| b64.encode(b)) },
        actor,
    )
}

#[derive(Serialize)]
pub struct Entry {
    id: Uuid,
    kind: String,
    name: String,
    deleted_by: String,
    deleted_at: DateTime<Utc>,
    purge_at: DateTime<Utc>,
}

fn retention(s: &AppState) -> u32 {
    s.db.meta_get("recycle_bin_days").ok().flatten().and_then(|d| d.parse().ok()).unwrap_or(DEFAULT_RETENTION_DAYS)
}

pub async fn list(State(s): State<AppState>, _a: Admin) -> ApiResult<Json<Vec<Entry>>> {
    let days = retention(&s) as i64;
    Ok(Json(
        s.db.list_recycled()?
            .into_iter()
            .map(|r| Entry {
                id: r.id,
                kind: r.kind,
                name: r.name,
                deleted_by: r.deleted_by,
                purge_at: r.deleted_at + chrono::Duration::days(days),
                deleted_at: r.deleted_at,
            })
            .collect(),
    ))
}

#[derive(Serialize)]
pub struct Restored {
    message: String,
    warnings: Vec<String>,
}

pub async fn restore(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<Restored>> {
    let r = s.db.get_recycled(id)?.ok_or_else(|| ApiError::not_found("Deleted object"))?;
    let snap: Snapshot = serde_json::from_str(&r.data).map_err(|_| ApiError::bad_request("This entry is damaged and can't be restored"))?;
    let mut warnings = vec![];
    let ous: std::collections::HashSet<Uuid> = s.db.list_ous()?.into_iter().map(|o| o.id).collect();
    let fix_ou = |ou: Option<Uuid>, warnings: &mut Vec<String>| match ou {
        Some(o) if !ous.contains(&o) => {
            warnings.push("Its OU no longer exists, so it was restored to the domain root.".into());
            None
        }
        x => x,
    };
    match snap {
        Snapshot::User { mut user, password_hash, password_sealed } => {
            if s.db.get_user_by_username(&user.username)?.is_some() {
                return Err(ApiError::conflict(format!("A user named {} already exists", user.username)));
            }
            user.ou_id = fix_ou(user.ou_id, &mut warnings);
            let groups = user.group_ids.clone();
            s.db.insert_user(&user)?;
            if let (Some(h), Some(sealed)) = (password_hash, password_sealed) {
                let bytes = base64::engine::general_purpose::STANDARD.decode(sealed).unwrap_or_default();
                s.db.set_user_password(user.id, &h, &bytes)?;
            }
            let existing: Vec<Uuid> = groups.into_iter().filter(|g| s.db.get_group(*g).ok().flatten().is_some()).collect();
            s.db.set_user_groups(user.id, &existing)?;
        }
        Snapshot::Group { mut group } => {
            if s.db.list_groups()?.iter().any(|g| g.name.eq_ignore_ascii_case(&group.name)) {
                return Err(ApiError::conflict(format!("A group named {} already exists", group.name)));
            }
            group.ou_id = fix_ou(group.ou_id, &mut warnings);
            s.db.insert_group(&group)?;
            let users: Vec<Uuid> = group.member_ids.iter().copied().filter(|u| s.db.get_user(*u).ok().flatten().is_some()).collect();
            let eps: Vec<Uuid> = group.endpoint_ids.iter().copied().filter(|e| s.db.get_endpoint(*e).ok().flatten().is_some()).collect();
            if users.len() < group.member_ids.len() {
                warnings.push(format!("{} member(s) no longer exist and were left out.", group.member_ids.len() - users.len()));
            }
            s.db.set_group_members(group.id, &users)?;
            s.db.set_group_endpoints(group.id, &eps)?;
        }
        Snapshot::Ou { mut ou } => {
            ou.parent_id = fix_ou(ou.parent_id, &mut warnings);
            s.db.insert_ou(&ou)?;
        }
        Snapshot::Policy { mut policy } => {
            let before = policy.targets.ou_ids.len();
            policy.targets.ou_ids.retain(|o| ous.contains(o));
            if policy.targets.ou_ids.len() < before {
                warnings.push("Some OU links pointed at OUs that no longer exist and were removed.".into());
            }
            s.db.upsert_policy(&policy)?;
        }
        Snapshot::Script { script } => {
            if s.db.list_scripts()?.iter().any(|x| x.name.eq_ignore_ascii_case(&script.name)) {
                return Err(ApiError::conflict(format!("A script named {} already exists", script.name)));
            }
            s.db.upsert_script(&script)?;
        }
    }
    s.db.delete_recycled(id)?;
    s.db.audit(admin.name(), "recycle_bin.restore", &r.name, &r.kind);
    Ok(Json(Restored { message: format!("Restored {}", r.name), warnings }))
}

pub async fn purge(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let r = s.db.get_recycled(id)?.ok_or_else(|| ApiError::not_found("Deleted object"))?;
    s.db.delete_recycled(id)?;
    s.db.audit(admin.name(), "recycle_bin.purge", &r.name, &r.kind);
    Ok(StatusCode::NO_CONTENT)
}

/// Remove entries past the retention period. Called daily.
pub fn expire(s: &AppState) -> anyhow::Result<usize> {
    s.db.purge_recycled_before(Utc::now() - chrono::Duration::days(retention(s) as i64))
}
