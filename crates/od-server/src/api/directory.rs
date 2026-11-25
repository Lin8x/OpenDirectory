//! Setup, sign-in, OUs, users, security groups, search, overview and audit.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{Duration, Utc};
use od_core::policy::{Policy, PolicyTargets, Setting};
use od_core::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::{required, validate_password, validate_username};
use crate::auth::Admin;
use crate::crypto::{hash_password, random_token, sha256_hex, verify_password};
use crate::error::{ApiError, ApiResult};
use crate::AppState;

const SESSION_HOURS: i64 = 12;

pub async fn health(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "status": "ok", "version": s.info.version }))
}

// ---------------------------------------------------------------- setup

#[derive(Serialize)]
pub struct SetupStatus {
    needs_setup: bool,
    organization: Option<String>,
    domain: Option<String>,
    server: crate::ServerInfo,
}

pub async fn setup_status(State(s): State<AppState>) -> ApiResult<Json<SetupStatus>> {
    Ok(Json(SetupStatus {
        needs_setup: s.db.count_users()? == 0,
        organization: s.db.meta_get("organization")?,
        domain: s.db.meta_get("domain")?,
        server: (*s.info).clone(),
    }))
}

#[derive(Deserialize)]
pub struct SetupRequest {
    organization: String,
    domain: String,
    admin_username: String,
    admin_display_name: String,
    admin_password: String,
    #[serde(default = "yes")]
    create_defaults: bool,
}
fn yes() -> bool {
    true
}

#[derive(Serialize)]
pub struct SessionResponse {
    token: String,
    user: User,
    expires_at: chrono::DateTime<Utc>,
}

fn validate_domain(d: &str) -> ApiResult<String> {
    let d = d.trim().trim_end_matches('.').to_lowercase();
    let ok = !d.is_empty()
        && d.len() <= 253
        && d.split('.').count() >= 2
        && d.split('.').all(|l| {
            !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if !ok {
        return Err(ApiError::bad_request("Enter a DNS domain name such as corp.example.com"));
    }
    Ok(d)
}

/// First-run setup: names the domain and creates the first Domain Admin.
/// Only works while the directory is empty.
pub async fn setup(State(s): State<AppState>, Json(req): Json<SetupRequest>) -> ApiResult<(StatusCode, Json<SessionResponse>)> {
    if s.db.count_users()? > 0 {
        return Err(ApiError::conflict("Setup has already been completed"));
    }
    let org = required("Organization name", &req.organization)?;
    let domain = validate_domain(&req.domain)?;
    let username = validate_username(&req.admin_username)?;
    validate_password(&req.admin_password)?;
    let display = required("Display name", &req.admin_display_name)?;

    s.db.meta_set("organization", &org)?;
    s.db.meta_set("domain", &domain)?;
    s.db.meta_set("setup_at", &Utc::now().to_rfc3339())?;

    let now = Utc::now();
    let mut admin_ou = None;
    let mut admin_groups = vec![];
    if req.create_defaults {
        let mk_ou = |name: &str, desc: &str| -> ApiResult<Uuid> {
            let ou = OrgUnit { id: Uuid::new_v4(), name: name.into(), description: desc.into(), parent_id: None, created_at: now };
            s.db.insert_ou(&ou)?;
            Ok(ou.id)
        };
        let staff = mk_ou("Staff", "People in the organization")?;
        mk_ou("Workstations", "Desktops and laptops")?;
        mk_ou("Servers", "Servers and infrastructure")?;
        admin_ou = Some(mk_ou("IT", "IT administrators")?);
        let _ = staff;

        for (name, desc) in [
            ("Domain Admins", "Full control of the directory and every endpoint"),
            ("Domain Users", "Every user account"),
        ] {
            let g = Group { id: Uuid::new_v4(), name: name.into(), description: desc.into(), ou_id: None, member_ids: vec![], created_at: now };
            s.db.insert_group(&g)?;
            admin_groups.push(g.id);
        }

        let policy = Policy {
            id: Uuid::new_v4(),
            name: "Default Endpoint Policy".into(),
            description: "Security baseline for every endpoint, like the Default Domain Policy in AD.".into(),
            enabled: true,
            priority: 1000,
            targets: PolicyTargets { all_endpoints: true, ou_ids: vec![] },
            settings: vec![
                Setting::Firewall { enabled: true },
                Setting::ScreenLock { minutes: 15 },
                Setting::DiskEncryption { required: true },
                Setting::AutomaticUpdates { enabled: true },
            ],
            created_at: now,
            updated_at: now,
        };
        s.db.upsert_policy(&policy)?;
    }

    let user = User {
        id: Uuid::new_v4(),
        username: username.clone(),
        display_name: display,
        email: String::new(),
        title: "Administrator".into(),
        department: "IT".into(),
        ou_id: admin_ou,
        enabled: true,
        role: Role::Admin,
        must_change_password: false,
        password_set_at: None,
        last_login_at: None,
        created_at: now,
        group_ids: vec![],
    };
    s.db.insert_user(&user)?;
    s.db.set_user_password(user.id, &hash_password(&req.admin_password)?, &s.sealer.seal(req.admin_password.as_bytes())?)?;
    if !admin_groups.is_empty() {
        s.db.set_user_groups(user.id, &admin_groups)?;
    }
    s.db.audit(&username, "domain.setup", &domain, &format!("Organization \"{org}\" created"));

    let session = new_session(&s, user.id)?;
    Ok((StatusCode::CREATED, Json(session)))
}

fn new_session(s: &AppState, user_id: Uuid) -> ApiResult<SessionResponse> {
    let token = random_token(32);
    let expires_at = Utc::now() + Duration::hours(SESSION_HOURS);
    s.db.insert_session(&sha256_hex(token.as_bytes()), user_id, expires_at)?;
    s.db.touch_login(user_id)?;
    let user = s.db.get_user(user_id)?.ok_or_else(|| ApiError::not_found("User"))?;
    Ok(SessionResponse { token, user, expires_at })
}

// ---------------------------------------------------------------- auth

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

pub async fn login(State(s): State<AppState>, Json(req): Json<LoginRequest>) -> ApiResult<Json<SessionResponse>> {
    let fail = || ApiError::new(StatusCode::UNAUTHORIZED, "Incorrect username or password");
    let name = req.username.trim().to_lowercase();
    // Accept DOMAIN\user and user@domain forms, like Windows does.
    let name = name.rsplit('\\').next().unwrap_or(&name).to_string();
    let name = name.split('@').next().unwrap_or(&name).to_string();

    let Some(user) = s.db.get_user_by_username(&name)? else {
        // Spend comparable time so usernames can't be probed by timing.
        static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let dummy = DUMMY.get_or_init(|| hash_password("timing-equalizer").unwrap_or_default());
        let _ = verify_password(&req.password, dummy);
        s.db.audit(&name, "auth.failed", &name, "Unknown account");
        return Err(fail());
    };
    let secrets = s.db.user_secrets(user.id)?;
    let ok = secrets.password_hash.as_deref().map(|h| verify_password(&req.password, h)).unwrap_or(false);
    if !ok {
        s.db.audit(&user.username, "auth.failed", &user.username, "Wrong password");
        return Err(fail());
    }
    if !user.enabled {
        s.db.audit(&user.username, "auth.failed", &user.username, "Account disabled");
        return Err(ApiError::forbidden("This account is disabled"));
    }
    if user.role != Role::Admin {
        return Err(ApiError::forbidden("Only Domain Admins can sign in to the Admin Center"));
    }
    s.db.audit(&user.username, "auth.login", &user.username, "");
    Ok(Json(new_session(&s, user.id)?))
}

pub async fn logout(State(s): State<AppState>, admin: Admin) -> ApiResult<StatusCode> {
    s.db.delete_session(&admin.token_hash)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(State(s): State<AppState>, admin: Admin) -> ApiResult<Json<Value>> {
    Ok(Json(json!({
        "user": admin.user,
        "organization": s.db.meta_get("organization")?,
        "domain": s.db.meta_get("domain")?,
        "server": *s.info,
    })))
}

// ---------------------------------------------------------------- overview

pub async fn overview(State(s): State<AppState>, _admin: Admin) -> ApiResult<Json<Value>> {
    let users = s.db.list_users()?;
    let groups = s.db.list_groups()?;
    let ous = s.db.list_ous()?;
    let endpoints = s.db.list_endpoints()?;
    let policies = s.db.list_policies()?;
    let jobs = s.db.list_jobs(None, 200)?;

    let mut by_os: HashMap<&str, usize> = HashMap::new();
    let mut noncompliant = 0;
    let mut stale = 0;
    for e in &endpoints {
        *by_os.entry(e.os.label()).or_default() += 1;
        if e.compliance.iter().any(|c| matches!(c.state, ComplianceState::NonCompliant | ComplianceState::Failed)) {
            noncompliant += 1;
        }
        if e.last_seen_at.map(|t| Utc::now() - t > Duration::days(14)).unwrap_or(true) {
            stale += 1;
        }
    }
    let no_password = users.iter().filter(|u| u.password_set_at.is_none()).count();
    let custom_policy = policies.iter().any(|p| p.name != "Default Endpoint Policy");
    let has_account_policy = policies.iter().any(|p| p.settings.iter().any(|s| matches!(s, Setting::LocalAccounts { .. })));

    Ok(Json(json!({
        "organization": s.db.meta_get("organization")?,
        "domain": s.db.meta_get("domain")?,
        "counts": {
            "users": users.len(),
            "users_enabled": users.iter().filter(|u| u.enabled).count(),
            "admins": users.iter().filter(|u| u.role == Role::Admin).count(),
            "groups": groups.len(),
            "ous": ous.len(),
            "endpoints": endpoints.len(),
            "endpoints_online": endpoints.iter().filter(|e| e.online).count(),
            "endpoints_noncompliant": noncompliant,
            "endpoints_stale": stale,
            "policies": policies.len(),
            "jobs_running": jobs.iter().filter(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running)).count(),
            "users_without_password": no_password,
        },
        "endpoints_by_os": by_os,
        "getting_started": {
            "created_user": users.len() > 1,
            "created_group": groups.iter().any(|g| g.name != "Domain Admins" && g.name != "Domain Users"),
            "enrolled_endpoint": !endpoints.is_empty(),
            "created_policy": custom_policy,
            "configured_sign_in": has_account_policy,
            "ran_script": !jobs.is_empty(),
        },
        "recent_activity": s.db.list_audit(8, None)?,
        "server": *s.info,
    })))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    q: String,
}

pub async fn search(State(s): State<AppState>, _admin: Admin, Query(q): Query<SearchQuery>) -> ApiResult<Json<Vec<Value>>> {
    let needle = q.q.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(Json(vec![]));
    }
    let hit = |hay: &[&str]| hay.iter().any(|h| h.to_lowercase().contains(&needle));
    let mut out = Vec::new();
    for u in s.db.list_users()? {
        if hit(&[&u.username, &u.display_name, &u.email, &u.department, &u.title]) {
            out.push(json!({"type": "user", "id": u.id, "title": u.display_name, "subtitle": u.username}));
        }
    }
    for g in s.db.list_groups()? {
        if hit(&[&g.name, &g.description]) {
            out.push(json!({"type": "group", "id": g.id, "title": g.name, "subtitle": format!("{} members", g.member_ids.len())}));
        }
    }
    for e in s.db.list_endpoints()? {
        let ips: Vec<&str> = e.inventory.interfaces.iter().flat_map(|i| i.ips.iter().map(|s| s.as_str())).collect();
        let macs: Vec<&str> = e.inventory.interfaces.iter().map(|i| i.mac.as_str()).collect();
        let mut hay = vec![e.hostname.as_str(), e.inventory.serial_number.as_str(), e.inventory.model.as_str(), e.public_ip.as_str()];
        hay.extend(ips);
        hay.extend(macs);
        if hit(&hay) {
            out.push(json!({"type": "endpoint", "id": e.id, "title": e.hostname, "subtitle": format!("{} · {}", e.os.label(), e.inventory.os_version)}));
        }
    }
    for o in s.db.list_ous()? {
        if hit(&[&o.name, &o.description]) {
            out.push(json!({"type": "ou", "id": o.id, "title": o.name, "subtitle": "Organizational unit"}));
        }
    }
    for p in s.db.list_policies()? {
        if hit(&[&p.name, &p.description]) {
            out.push(json!({"type": "policy", "id": p.id, "title": p.name, "subtitle": "Group Policy Object"}));
        }
    }
    out.truncate(50);
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct AuditQuery {
    q: Option<String>,
    limit: Option<u32>,
}

pub async fn audit(State(s): State<AppState>, _admin: Admin, Query(q): Query<AuditQuery>) -> ApiResult<Json<Vec<AuditEntry>>> {
    Ok(Json(s.db.list_audit(q.limit.unwrap_or(200).min(2000), q.q.as_deref())?))
}

// ---------------------------------------------------------------- OUs

#[derive(Deserialize)]
pub struct OuInput {
    name: Option<String>,
    description: Option<String>,
    #[serde(default, with = "double_option")]
    parent_id: Option<Option<Uuid>>,
}

pub async fn list_ous(State(s): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<OrgUnit>>> {
    Ok(Json(s.db.list_ous()?))
}

pub async fn get_ou(State(s): State<AppState>, _admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<OrgUnit>> {
    Ok(Json(s.db.get_ou(id)?.ok_or_else(|| ApiError::not_found("OU"))?))
}

fn check_parent(s: &AppState, id: Option<Uuid>, parent: Option<Uuid>) -> ApiResult<()> {
    let Some(p) = parent else { return Ok(()) };
    s.db.get_ou(p)?.ok_or_else(|| ApiError::bad_request("Parent OU does not exist"))?;
    // Walk up from the new parent; finding ourselves means a cycle.
    if let Some(me) = id {
        let all: HashMap<Uuid, Option<Uuid>> = s.db.list_ous()?.into_iter().map(|o| (o.id, o.parent_id)).collect();
        let mut cur = Some(p);
        while let Some(c) = cur {
            if c == me {
                return Err(ApiError::bad_request("An OU can't be moved inside itself"));
            }
            cur = all.get(&c).copied().flatten();
        }
    }
    Ok(())
}

pub async fn create_ou(State(s): State<AppState>, admin: Admin, Json(inp): Json<OuInput>) -> ApiResult<(StatusCode, Json<OrgUnit>)> {
    let name = required("Name", inp.name.as_deref().unwrap_or(""))?;
    let parent = inp.parent_id.flatten();
    check_parent(&s, None, parent)?;
    let ou = OrgUnit { id: Uuid::new_v4(), name, description: inp.description.unwrap_or_default(), parent_id: parent, created_at: Utc::now() };
    s.db.insert_ou(&ou)?;
    s.db.audit(admin.name(), "ou.create", &ou.name, "");
    Ok((StatusCode::CREATED, Json(ou)))
}

pub async fn update_ou(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>, Json(inp): Json<OuInput>) -> ApiResult<Json<OrgUnit>> {
    let mut ou = s.db.get_ou(id)?.ok_or_else(|| ApiError::not_found("OU"))?;
    if let Some(n) = inp.name {
        ou.name = required("Name", &n)?;
    }
    if let Some(d) = inp.description {
        ou.description = d;
    }
    if let Some(p) = inp.parent_id {
        check_parent(&s, Some(id), p)?;
        ou.parent_id = p;
    }
    s.db.update_ou(&ou)?;
    s.db.audit(admin.name(), "ou.update", &ou.name, "");
    Ok(Json(ou))
}

pub async fn delete_ou(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let ou = s.db.get_ou(id)?.ok_or_else(|| ApiError::not_found("OU"))?;
    let (children, users, groups, endpoints) = s.db.ou_usage(id)?;
    if children + users + groups + endpoints > 0 {
        return Err(ApiError::conflict(format!(
            "Move its contents first: {children} child OUs, {users} users, {groups} groups, {endpoints} endpoints"
        )));
    }
    s.db.delete_ou(id)?;
    s.db.audit(admin.name(), "ou.delete", &ou.name, "");
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- users

#[derive(Deserialize)]
pub struct UserInput {
    username: Option<String>,
    display_name: Option<String>,
    email: Option<String>,
    title: Option<String>,
    department: Option<String>,
    #[serde(default, with = "double_option")]
    ou_id: Option<Option<Uuid>>,
    enabled: Option<bool>,
    role: Option<Role>,
    must_change_password: Option<bool>,
    /// Only on create.
    password: Option<String>,
    /// Only on create.
    group_ids: Option<Vec<Uuid>>,
}

pub async fn list_users(State(s): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<User>>> {
    Ok(Json(s.db.list_users()?))
}

pub async fn get_user(State(s): State<AppState>, _admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<User>> {
    Ok(Json(s.db.get_user(id)?.ok_or_else(|| ApiError::not_found("User"))?))
}

fn check_ou(s: &AppState, ou: Option<Uuid>) -> ApiResult<()> {
    if let Some(o) = ou {
        s.db.get_ou(o)?.ok_or_else(|| ApiError::bad_request("That OU does not exist"))?;
    }
    Ok(())
}

pub async fn create_user(State(s): State<AppState>, admin: Admin, Json(inp): Json<UserInput>) -> ApiResult<(StatusCode, Json<User>)> {
    let username = validate_username(inp.username.as_deref().unwrap_or(""))?;
    let display_name = match inp.display_name.as_deref().map(str::trim) {
        Some(d) if !d.is_empty() => d.to_string(),
        _ => username.clone(),
    };
    let ou_id = inp.ou_id.flatten();
    check_ou(&s, ou_id)?;
    if let Some(pw) = &inp.password {
        validate_password(pw)?;
    }
    let user = User {
        id: Uuid::new_v4(),
        username: username.clone(),
        display_name,
        email: inp.email.unwrap_or_default().trim().to_string(),
        title: inp.title.unwrap_or_default(),
        department: inp.department.unwrap_or_default(),
        ou_id,
        enabled: inp.enabled.unwrap_or(true),
        role: inp.role.unwrap_or_default(),
        must_change_password: inp.must_change_password.unwrap_or(false),
        password_set_at: None,
        last_login_at: None,
        created_at: Utc::now(),
        group_ids: vec![],
    };
    s.db.insert_user(&user)?;
    if let Some(pw) = &inp.password {
        s.db.set_user_password(user.id, &hash_password(pw)?, &s.sealer.seal(pw.as_bytes())?)?;
    }
    let mut groups = inp.group_ids.unwrap_or_default();
    if let Some(du) = s.db.list_groups()?.into_iter().find(|g| g.name == "Domain Users") {
        if !groups.contains(&du.id) {
            groups.push(du.id);
        }
    }
    s.db.set_user_groups(user.id, &groups)?;
    s.db.audit(admin.name(), "user.create", &username, if user.role == Role::Admin { "Role: Admin" } else { "" });
    Ok((StatusCode::CREATED, Json(s.db.get_user(user.id)?.expect("just inserted"))))
}

pub async fn update_user(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>, Json(inp): Json<UserInput>) -> ApiResult<Json<User>> {
    let mut u = s.db.get_user(id)?.ok_or_else(|| ApiError::not_found("User"))?;
    let was_admin = u.role == Role::Admin && u.enabled;
    let mut changes = vec![];
    if let Some(n) = inp.username {
        let n = validate_username(&n)?;
        if n != u.username {
            changes.push(format!("renamed from {}", u.username));
            u.username = n;
        }
    }
    if let Some(v) = inp.display_name {
        u.display_name = required("Display name", &v)?;
    }
    if let Some(v) = inp.email {
        u.email = v.trim().into();
    }
    if let Some(v) = inp.title {
        u.title = v;
    }
    if let Some(v) = inp.department {
        u.department = v;
    }
    if let Some(v) = inp.ou_id {
        check_ou(&s, v)?;
        u.ou_id = v;
    }
    if let Some(v) = inp.enabled {
        if v != u.enabled {
            changes.push(if v { "enabled".into() } else { "disabled".into() });
        }
        u.enabled = v;
    }
    if let Some(v) = inp.role {
        if v != u.role {
            changes.push(format!("role {:?}", v).to_lowercase());
        }
        u.role = v;
    }
    if let Some(v) = inp.must_change_password {
        u.must_change_password = v;
    }
    let still_admin = u.role == Role::Admin && u.enabled;
    if was_admin && !still_admin && s.db.count_enabled_admins()? <= 1 {
        return Err(ApiError::conflict("This is the last enabled Domain Admin; add another admin first"));
    }
    s.db.update_user(&u)?;
    if !u.enabled {
        s.db.delete_user_sessions(u.id)?;
    }
    s.db.audit(admin.name(), "user.update", &u.username, &changes.join(", "));
    Ok(Json(s.db.get_user(id)?.expect("exists")))
}

pub async fn delete_user(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let u = s.db.get_user(id)?.ok_or_else(|| ApiError::not_found("User"))?;
    if u.id == admin.user.id {
        return Err(ApiError::conflict("You can't delete your own account"));
    }
    if u.role == Role::Admin && u.enabled && s.db.count_enabled_admins()? <= 1 {
        return Err(ApiError::conflict("This is the last enabled Domain Admin"));
    }
    s.db.delete_user(id)?;
    s.db.audit(admin.name(), "user.delete", &u.username, "");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PasswordInput {
    password: String,
    #[serde(default)]
    must_change_password: bool,
}

pub async fn reset_password(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>, Json(inp): Json<PasswordInput>) -> ApiResult<StatusCode> {
    let mut u = s.db.get_user(id)?.ok_or_else(|| ApiError::not_found("User"))?;
    validate_password(&inp.password)?;
    s.db.set_user_password(id, &hash_password(&inp.password)?, &s.sealer.seal(inp.password.as_bytes())?)?;
    u.must_change_password = inp.must_change_password;
    s.db.update_user(&u)?;
    if u.id != admin.user.id {
        s.db.delete_user_sessions(u.id)?;
    }
    s.db.audit(admin.name(), "user.password_reset", &u.username, "Synced to endpoints on next check-in");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct IdList {
    ids: Vec<Uuid>,
}

pub async fn set_user_groups(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>, Json(inp): Json<IdList>) -> ApiResult<Json<User>> {
    let u = s.db.get_user(id)?.ok_or_else(|| ApiError::not_found("User"))?;
    s.db.set_user_groups(id, &inp.ids)?;
    s.db.audit(admin.name(), "user.groups", &u.username, &format!("{} groups", inp.ids.len()));
    Ok(Json(s.db.get_user(id)?.expect("exists")))
}

// ---------------------------------------------------------------- groups

#[derive(Deserialize)]
pub struct GroupInput {
    name: Option<String>,
    description: Option<String>,
    #[serde(default, with = "double_option")]
    ou_id: Option<Option<Uuid>>,
    member_ids: Option<Vec<Uuid>>,
}

pub async fn list_groups(State(s): State<AppState>, _admin: Admin) -> ApiResult<Json<Vec<Group>>> {
    Ok(Json(s.db.list_groups()?))
}

pub async fn get_group(State(s): State<AppState>, _admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<Group>> {
    Ok(Json(s.db.get_group(id)?.ok_or_else(|| ApiError::not_found("Group"))?))
}

pub async fn create_group(State(s): State<AppState>, admin: Admin, Json(inp): Json<GroupInput>) -> ApiResult<(StatusCode, Json<Group>)> {
    let name = required("Name", inp.name.as_deref().unwrap_or(""))?;
    let ou_id = inp.ou_id.flatten();
    check_ou(&s, ou_id)?;
    let g = Group { id: Uuid::new_v4(), name, description: inp.description.unwrap_or_default(), ou_id, member_ids: vec![], created_at: Utc::now() };
    s.db.insert_group(&g)?;
    if let Some(m) = inp.member_ids {
        s.db.set_group_members(g.id, &m)?;
    }
    s.db.audit(admin.name(), "group.create", &g.name, "");
    Ok((StatusCode::CREATED, Json(s.db.get_group(g.id)?.expect("just inserted"))))
}

pub async fn update_group(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>, Json(inp): Json<GroupInput>) -> ApiResult<Json<Group>> {
    let mut g = s.db.get_group(id)?.ok_or_else(|| ApiError::not_found("Group"))?;
    if let Some(n) = inp.name {
        g.name = required("Name", &n)?;
    }
    if let Some(d) = inp.description {
        g.description = d;
    }
    if let Some(o) = inp.ou_id {
        check_ou(&s, o)?;
        g.ou_id = o;
    }
    s.db.update_group(&g)?;
    if let Some(m) = inp.member_ids {
        s.db.set_group_members(id, &m)?;
    }
    s.db.audit(admin.name(), "group.update", &g.name, "");
    Ok(Json(s.db.get_group(id)?.expect("exists")))
}

pub async fn set_group_members(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>, Json(inp): Json<IdList>) -> ApiResult<Json<Group>> {
    let g = s.db.get_group(id)?.ok_or_else(|| ApiError::not_found("Group"))?;
    s.db.set_group_members(id, &inp.ids)?;
    s.db.audit(admin.name(), "group.members", &g.name, &format!("{} members", inp.ids.len()));
    Ok(Json(s.db.get_group(id)?.expect("exists")))
}

pub async fn delete_group(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let g = s.db.get_group(id)?.ok_or_else(|| ApiError::not_found("Group"))?;
    if g.name == "Domain Admins" || g.name == "Domain Users" {
        return Err(ApiError::conflict("Built-in groups can't be deleted"));
    }
    let used_by: Vec<String> = s
        .db
        .list_policies()?
        .into_iter()
        .filter(|p| p.settings.iter().any(|st| matches!(st, Setting::LocalAccounts { group_id, .. } if *group_id == id)))
        .map(|p| p.name)
        .collect();
    if !used_by.is_empty() {
        return Err(ApiError::conflict(format!("Used for sign-in accounts by: {}", used_by.join(", "))));
    }
    s.db.delete_group(id)?;
    s.db.audit(admin.name(), "group.delete", &g.name, "");
    Ok(StatusCode::NO_CONTENT)
}

/// Distinguish "field absent" from "field: null" in PATCH bodies.
pub(crate) mod double_option {
    use serde::{Deserialize, Deserializer};
    pub fn deserialize<'de, T, D>(d: D) -> Result<Option<Option<T>>, D::Error>
    where
        T: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        Option::<T>::deserialize(d).map(Some)
    }
}
