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
use crate::auth::{Admin, Operator};
use crate::crypto::{hash_password, random_token, sha256_hex, verify_password};
use crate::error::{ApiError, ApiResult};
use crate::rbac::Permission;
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
    /// Legacy flag from v0.2: true means the Small Business baseline.
    #[serde(default = "yes")]
    create_defaults: bool,
    /// Security baseline to start from; overrides `create_defaults`.
    #[serde(default)]
    baseline: Option<od_core::baselines::Baseline>,
    #[serde(default)]
    modifiers: od_core::baselines::Modifiers,
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
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
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
    let baseline = req.baseline.unwrap_or(if req.create_defaults {
        od_core::baselines::Baseline::SmallBusiness
    } else {
        od_core::baselines::Baseline::Custom
    });
    let plan = od_core::baselines::plan(baseline, &req.modifiers);
    // The first admin's password must already meet the baseline's rules.
    if let Some(min) = plan.policies.iter().flat_map(|p| &p.settings).find_map(|st| match st {
        Setting::PasswordMinLength { length } => Some(*length),
        _ => None,
    }) {
        if (req.admin_password.chars().count() as u32) < min {
            return Err(ApiError::bad_request(format!(
                "This baseline requires passwords of at least {min} characters; choose a longer admin password"
            )));
        }
    }
    let (admin_ou, admin_groups) = apply_plan(&s, &plan)?;

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

/// Create the OUs, groups and GPOs of a baseline plan. Returns the admin's
/// OU and the built-in groups the first admin joins.
fn apply_plan(s: &AppState, plan: &od_core::baselines::Plan) -> ApiResult<(Option<Uuid>, Vec<Uuid>)> {
    use od_core::baselines::Rollout;
    let now = Utc::now();
    let mut ou_ids: HashMap<String, Uuid> = HashMap::new();
    for o in &plan.ous {
        let parent = o.parent.as_ref().and_then(|p| ou_ids.get(p).copied());
        let ou =
            OrgUnit { id: Uuid::new_v4(), name: o.name.clone(), description: o.description.clone(), parent_id: parent, created_at: now };
        s.db.insert_ou(&ou)?;
        ou_ids.insert(o.name.clone(), ou.id);
    }
    let mut group_ids: HashMap<String, Uuid> = HashMap::new();
    let builtin = [(DOMAIN_ADMINS, "Full control of the directory and every endpoint"), ("Domain Users", "Every user account")];
    for (name, desc) in builtin.iter().copied().chain(plan.groups.iter().map(|g| (g.name.as_str(), g.description.as_str()))) {
        if group_ids.contains_key(name) {
            continue;
        }
        let g = Group {
            id: Uuid::new_v4(),
            name: name.into(),
            description: desc.into(),
            ou_id: None,
            member_ids: vec![],
            endpoint_ids: vec![],
            created_at: now,
        };
        s.db.insert_group(&g)?;
        group_ids.insert(name.to_string(), g.id);
    }
    let mut policies: Vec<Policy> = plan
        .policies
        .iter()
        .map(|p| Policy {
            id: Uuid::new_v4(),
            name: p.name.clone(),
            description: p.description.clone(),
            enabled: p.enabled,
            priority: p.priority,
            targets: PolicyTargets {
                all_endpoints: p.all_endpoints,
                ou_ids: p.ou_links.iter().filter_map(|n| ou_ids.get(n).copied()).collect(),
                ..Default::default()
            },
            settings: p.settings.clone(),
            created_at: now,
            updated_at: now,
        })
        .collect();
    if let Some(gid) = plan.signin_group.as_ref().and_then(|g| group_ids.get(g)) {
        let pilot = plan.rollout == Rollout::Pilot;
        policies.push(Policy {
            id: Uuid::new_v4(),
            name: "Sign-in Accounts".into(),
            description: "Members of this group can sign in to managed computers with their directory username and password.".into(),
            enabled: plan.rollout != Rollout::Review,
            priority: 500,
            targets: PolicyTargets {
                all_endpoints: !pilot,
                ou_ids: if pilot { ou_ids.get("Pilot").copied().into_iter().collect() } else { vec![] },
                ..Default::default()
            },
            settings: vec![Setting::LocalAccounts { group_id: *gid, admin: plan.signin_admin }],
            created_at: now,
            updated_at: now,
        });
    }
    for p in &policies {
        s.db.upsert_policy(p)?;
    }
    // Directory passwords follow the same rules the baseline sets for endpoints.
    if let Some(def) = plan.policies.iter().find(|p| p.name == "Default Endpoint Policy") {
        let mut r = super::passwords::Rules::default();
        for st in &def.settings {
            match st {
                Setting::PasswordMinLength { length } => r.min_length = *length,
                Setting::PasswordComplexity { required } => r.complexity = *required,
                Setting::PasswordHistory { count } => r.history = *count,
                Setting::PasswordMaxAge { days } => r.max_age_days = *days,
                Setting::AccountLockout { threshold, duration_minutes } => {
                    r.lockout_threshold = *threshold;
                    r.lockout_minutes = *duration_minutes;
                }
                _ => {}
            }
        }
        s.db.meta_set("password_policy", &serde_json::to_string(&r).map_err(anyhow::Error::from)?)?;
    }
    let tag = |v: serde_json::Result<serde_json::Value>| v.ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    s.db.meta_set("baseline", &tag(serde_json::to_value(plan.baseline)))?;
    if let Some(l) = plan.level {
        s.db.meta_set("security_level", &tag(serde_json::to_value(l)))?;
    }
    if let Some(d) = plan.audit_retention_days {
        s.db.meta_set("audit_retention_days", &d.to_string())?;
    }
    if let Some(n) = &plan.naming_convention {
        s.db.meta_set("naming_convention", n)?;
    }
    let admin_ou = plan.admin_ou.as_ref().and_then(|n| ou_ids.get(n).copied());
    let mut admin_groups: Vec<Uuid> = [DOMAIN_ADMINS, "Domain Users"].iter().filter_map(|n| group_ids.get(*n).copied()).collect();
    if let Some(t0) = group_ids.get("Tier 0 Admins") {
        admin_groups.push(*t0);
    }
    Ok((admin_ou, admin_groups))
}

#[derive(Deserialize)]
pub struct BaselinePreview {
    baseline: od_core::baselines::Baseline,
    #[serde(default)]
    modifiers: od_core::baselines::Modifiers,
}

#[derive(Serialize)]
pub struct BaselineCatalog {
    baselines: Vec<od_core::baselines::BaselineInfo>,
    levels: Vec<(od_core::baselines::SecurityLevel, String)>,
}

/// Baselines and security levels, for the onboarding wizard.
pub async fn baselines() -> Json<BaselineCatalog> {
    use od_core::baselines::SecurityLevel::*;
    Json(BaselineCatalog {
        baselines: od_core::baselines::catalog(),
        levels: [Relaxed, Balanced, Strict, Maximum].into_iter().map(|l| (l, od_core::baselines::level_summary(l))).collect(),
    })
}

/// Exactly what a baseline plus modifiers would create. Pure computation, so
/// it works before setup.
pub async fn preview_baseline(Json(req): Json<BaselinePreview>) -> Json<od_core::baselines::Plan> {
    Json(od_core::baselines::plan(req.baseline, &req.modifiers))
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
    /// Six-digit authenticator code, for accounts with MFA.
    #[serde(default)]
    otp: Option<String>,
}

/// Lockout rules for a username: its resultant password policy, or the
/// domain default for unknown names (AD's account lockout threshold and duration).
fn lockout_rules(s: &AppState, name: &str) -> (u32, std::time::Duration) {
    let uid = s.db.get_user_by_username(name).ok().flatten().map(|u| u.id);
    let r = super::passwords::resultant(s, uid).rules;
    (r.lockout_threshold, std::time::Duration::from_secs(r.lockout_minutes.max(1) as u64 * 60))
}

/// How long this username is still locked out, if it is.
pub(crate) fn locked_out(s: &AppState, name: &str) -> Option<std::time::Duration> {
    let (threshold, window) = lockout_rules(s, name);
    if threshold == 0 {
        return None;
    }
    let mut f = s.login_failures.lock().unwrap_or_else(|e| e.into_inner());
    f.retain(|_, (_, at)| at.elapsed() < std::time::Duration::from_secs(24 * 3600));
    f.get(name).filter(|(n, at)| *n >= threshold && at.elapsed() < window).map(|(_, at)| window.saturating_sub(at.elapsed()))
}

pub(crate) fn record_failed_login(s: &AppState, name: &str, reason: &str) {
    let (threshold, window) = lockout_rules(s, name);
    let mut f = s.login_failures.lock().unwrap_or_else(|e| e.into_inner());
    let e = f.entry(name.to_string()).or_insert((0, std::time::Instant::now()));
    if e.1.elapsed() >= window {
        e.0 = 0;
    }
    e.0 += 1;
    e.1 = std::time::Instant::now();
    let detail = if threshold > 0 && e.0 >= threshold {
        format!("{reason}; locked out for {} minutes", window.as_secs() / 60)
    } else {
        reason.to_string()
    };
    drop(f);
    s.db.audit(name, "auth.failed", "", &detail);
}

pub async fn login(State(s): State<AppState>, Json(req): Json<LoginRequest>) -> ApiResult<Json<SessionResponse>> {
    let fail = || ApiError::new(StatusCode::UNAUTHORIZED, "Incorrect username or password");
    let name = req.username.trim().to_lowercase();
    // Accept DOMAIN\user and user@domain forms, like Windows does.
    let name = name.rsplit('\\').next().unwrap_or(&name).to_string();
    let name = name.split('@').next().unwrap_or(&name).to_string();

    if let Some(left) = locked_out(&s, &name) {
        let mins = left.as_secs().div_ceil(60).max(1);
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            format!("Too many failed attempts. Try again in {mins} minute{}.", if mins == 1 { "" } else { "s" }),
        ));
    }
    let record_failure = |reason: &str| record_failed_login(&s, &name, reason);

    let Some(user) = s.db.get_user_by_username(&name)? else {
        // Spend comparable time so usernames can't be probed by timing.
        static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let dummy = DUMMY.get_or_init(|| hash_password("timing-equalizer").unwrap_or_default());
        let _ = verify_password(&req.password, dummy);
        record_failure("Unknown account");
        return Err(fail());
    };
    let secrets = s.db.user_secrets(user.id)?;
    let ok = secrets.password_hash.as_deref().map(|h| verify_password(&req.password, h)).unwrap_or(false);
    if !ok {
        record_failure("Wrong password");
        return Err(fail());
    }
    if !user.enabled {
        s.db.audit(&user.username, "auth.failed", "", "Account disabled");
        return Err(ApiError::forbidden("This account is disabled"));
    }
    if user.role != Role::Admin && crate::rbac::for_user(&s, &user).is_empty() {
        s.db.audit(&user.username, "auth.failed", "", "No Admin Center access");
        return Err(ApiError::forbidden(
            "This account has no Admin Center access. Ask a Domain Admin to delegate endpoints or accounts to you.",
        ));
    }
    if super::passwords::expired(&s, &user) {
        s.db.audit(&user.username, "auth.failed", "", "Password expired");
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Your password has expired. Choose a new one with \"Change password\" on the sign-in screen.",
        ));
    }
    let mfa = super::mfa::enabled(&s, user.id);
    if mfa {
        match req.otp.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            None => {
                return Err(
                    ApiError::new(StatusCode::UNAUTHORIZED, "Enter the 6-digit code from your authenticator app").with_code("mfa_required")
                )
            }
            Some(code) => {
                if !super::mfa::verify(&s, user.id, code)? {
                    record_failure("Wrong MFA code");
                    return Err(ApiError::new(StatusCode::UNAUTHORIZED, "That code isn't right, or was already used. Try the next one.")
                        .with_code("mfa_required"));
                }
            }
        }
    }
    s.login_failures.lock().unwrap_or_else(|e| e.into_inner()).remove(&name);
    s.db.audit(&user.username, "auth.login", "", if mfa { "With MFA" } else { "" });
    Ok(Json(new_session(&s, user.id)?))
}

pub async fn logout(State(s): State<AppState>, admin: Operator) -> ApiResult<StatusCode> {
    s.db.delete_session(&admin.token_hash)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(State(s): State<AppState>, admin: Operator) -> ApiResult<Json<Value>> {
    let delegations: Vec<Value> = crate::rbac::for_user(&s, &admin.user)
        .iter()
        .map(|d| json!({ "id": d.id, "name": d.name, "permissions": d.permissions, "expires_at": d.expires_at }))
        .collect();
    Ok(Json(json!({
        "user": admin.user,
        "domain_admin": admin.access.domain_admin,
        "permissions": admin.access.permissions(),
        "delegations": delegations,
        "mfa_enabled": super::mfa::enabled(&s, admin.user.id),
        "mfa_enrollment_required": super::mfa::must_enroll(&s, admin.user.id, admin.access.domain_admin),
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

pub async fn list_ous(State(s): State<AppState>, op: Operator) -> ApiResult<Json<Vec<OrgUnit>>> {
    let mut ous = s.db.list_ous()?;
    if !op.access.domain_admin {
        ous.retain(|o| op.access.ou(o));
        // Top of the delegated tree shows as a root.
        let ids: Vec<Uuid> = ous.iter().map(|o| o.id).collect();
        for o in ous.iter_mut() {
            if o.parent_id.is_some_and(|p| !ids.contains(&p)) {
                o.parent_id = None;
            }
        }
    }
    Ok(Json(ous))
}

pub async fn get_ou(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<OrgUnit>> {
    let ou = s.db.get_ou(id)?.filter(|o| op.access.ou(o)).ok_or_else(|| ApiError::not_found("OU"))?;
    Ok(Json(ou))
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
    let ou =
        OrgUnit { id: Uuid::new_v4(), name, description: inp.description.unwrap_or_default(), parent_id: parent, created_at: Utc::now() };
    s.db.insert_ou(&ou)?;
    s.db.audit(admin.name(), "ou.create", &ou.name, "");
    Ok((StatusCode::CREATED, Json(ou)))
}

pub async fn update_ou(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<OuInput>,
) -> ApiResult<Json<OrgUnit>> {
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
    super::recycle::keep(&s, super::recycle::Snapshot::Ou { ou: ou.clone() }, admin.name())?;
    s.db.delete_ou(id)?;
    s.db.audit(admin.name(), "ou.delete", &ou.name, "Moved to the Recycle Bin");
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

pub async fn list_users(State(s): State<AppState>, op: Operator) -> ApiResult<Json<Vec<User>>> {
    let mut users = s.db.list_users()?;
    if !op.access.domain_admin {
        users.retain(|u| op.access.sees_user(u));
        scrub_user_groups(&s, &op, &mut users)?;
    }
    Ok(Json(users))
}

pub async fn get_user(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<User>> {
    let u = s.db.get_user(id)?.filter(|u| op.access.sees_user(u)).ok_or_else(|| ApiError::not_found("User"))?;
    let mut v = vec![u];
    if !op.access.domain_admin {
        scrub_user_groups(&s, &op, &mut v)?;
    }
    Ok(Json(v.remove(0)))
}

/// Delegated operators only see memberships of groups in their scope.
fn scrub_user_groups(s: &AppState, op: &Operator, users: &mut [User]) -> ApiResult<()> {
    let visible: Vec<Uuid> = s.db.list_groups()?.into_iter().filter(|g| op.access.group(g)).map(|g| g.id).collect();
    for u in users {
        u.group_ids.retain(|g| visible.contains(g));
    }
    Ok(())
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
        super::passwords::check(&s, None, pw)?;
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
    let mut user = user;
    if let Some(da) = domain_admins(&s)? {
        if groups.contains(&da.id) || user.role == Role::Admin {
            user.role = Role::Admin;
            s.db.update_user(&user)?;
            if !groups.contains(&da.id) {
                groups.push(da.id);
            }
        }
    }
    s.db.set_user_groups(user.id, &groups)?;
    s.db.audit(admin.name(), "user.create", &username, if user.role == Role::Admin { "Role: Admin" } else { "" });
    Ok((StatusCode::CREATED, Json(s.db.get_user(user.id)?.expect("just inserted"))))
}

pub async fn update_user(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<UserInput>,
) -> ApiResult<Json<User>> {
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
    sync_group_from_role(&s, &u)?;
    if !u.enabled || !still_admin {
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
    super::recycle::keep_user(&s, &u, admin.name())?;
    s.db.delete_user(id)?;
    s.db.audit(admin.name(), "user.delete", &u.username, "Moved to the Recycle Bin");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PasswordInput {
    password: String,
    #[serde(default)]
    must_change_password: bool,
}

pub async fn reset_password(
    State(s): State<AppState>,
    admin: Operator,
    Path(id): Path<Uuid>,
    Json(inp): Json<PasswordInput>,
) -> ApiResult<StatusCode> {
    let mut u = s.db.get_user(id)?.ok_or_else(|| ApiError::not_found("User"))?;
    admin.need_user(&u, Permission::ResetPasswords)?;
    super::passwords::check(&s, Some(id), &inp.password)?;
    super::passwords::set(&s, id, &inp.password)?;
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

pub async fn set_user_groups(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<IdList>,
) -> ApiResult<Json<User>> {
    let u = s.db.get_user(id)?.ok_or_else(|| ApiError::not_found("User"))?;
    if let Some(da) = domain_admins(&s)? {
        let mut members: Vec<Uuid> = da.member_ids.iter().copied().filter(|m| *m != id).collect();
        if inp.ids.contains(&da.id) {
            members.push(id);
        }
        apply_domain_admin_members(&s, &members)?;
    }
    s.db.set_user_groups(id, &inp.ids)?;
    s.db.audit(admin.name(), "user.groups", &u.username, &format!("{} groups", inp.ids.len()));
    Ok(Json(s.db.get_user(id)?.expect("exists")))
}

// ---------------------------------------------------------------- Domain Admins
//
// Like AD, being a Domain Admin and being a member of the Domain Admins group
// are the same thing. These helpers keep the user's role and the group's
// membership in step whichever side an admin edits.

const DOMAIN_ADMINS: &str = "Domain Admins";
const BUILTIN_GROUPS: [&str; 2] = [DOMAIN_ADMINS, "Domain Users"];

fn domain_admins(s: &AppState) -> ApiResult<Option<Group>> {
    Ok(s.db.list_groups()?.into_iter().find(|g| g.name == DOMAIN_ADMINS))
}

/// Role changed: add or remove the user from Domain Admins to match.
fn sync_group_from_role(s: &AppState, user: &User) -> ApiResult<()> {
    let Some(g) = domain_admins(s)? else { return Ok(()) };
    let mut members = g.member_ids.clone();
    let is_member = members.contains(&user.id);
    match (user.role == Role::Admin, is_member) {
        (true, false) => members.push(user.id),
        (false, true) => members.retain(|m| *m != user.id),
        _ => return Ok(()),
    }
    s.db.set_group_members(g.id, &members)?;
    Ok(())
}

/// Domain Admins membership is about to change: refuse if no enabled admin
/// would remain, otherwise give or take the Admin role to match.
fn apply_domain_admin_members(s: &AppState, members: &[Uuid]) -> ApiResult<()> {
    let users = s.db.list_users()?;
    if !users.iter().any(|u| u.enabled && members.contains(&u.id)) {
        return Err(ApiError::conflict("Domain Admins must keep at least one enabled member"));
    }
    for mut u in users {
        let want = members.contains(&u.id);
        if (u.role == Role::Admin) != want {
            u.role = if want { Role::Admin } else { Role::User };
            s.db.update_user(&u)?;
            if !want {
                s.db.delete_user_sessions(u.id)?;
            }
        }
    }
    Ok(())
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

pub async fn list_groups(State(s): State<AppState>, op: Operator) -> ApiResult<Json<Vec<Group>>> {
    let mut groups = s.db.list_groups()?;
    if !op.access.domain_admin {
        groups.retain(|g| op.access.group(g));
        scrub_group_members(&s, &op, &mut groups)?;
    }
    Ok(Json(groups))
}

pub async fn get_group(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>) -> ApiResult<Json<Group>> {
    let g = s.db.get_group(id)?.filter(|g| op.access.group(g)).ok_or_else(|| ApiError::not_found("Group"))?;
    let mut v = vec![g];
    if !op.access.domain_admin {
        scrub_group_members(&s, &op, &mut v)?;
    }
    Ok(Json(v.remove(0)))
}

/// Delegated operators only see members that are in their scope.
fn scrub_group_members(s: &AppState, op: &Operator, groups: &mut [Group]) -> ApiResult<()> {
    let users: Vec<Uuid> = s.db.list_users()?.into_iter().filter(|u| op.access.sees_user(u)).map(|u| u.id).collect();
    let eps: Vec<Uuid> = s.db.list_endpoints()?.into_iter().filter(|e| op.access.sees_endpoint(e)).map(|e| e.id).collect();
    for g in groups {
        g.member_ids.retain(|m| users.contains(m));
        g.endpoint_ids.retain(|m| eps.contains(m));
    }
    Ok(())
}

pub async fn create_group(State(s): State<AppState>, admin: Admin, Json(inp): Json<GroupInput>) -> ApiResult<(StatusCode, Json<Group>)> {
    let name = required("Name", inp.name.as_deref().unwrap_or(""))?;
    if s.db.list_groups()?.iter().any(|g| g.name.eq_ignore_ascii_case(&name)) {
        return Err(ApiError::conflict(format!("A group named \"{name}\" already exists")));
    }
    let ou_id = inp.ou_id.flatten();
    check_ou(&s, ou_id)?;
    let g = Group {
        id: Uuid::new_v4(),
        name,
        description: inp.description.unwrap_or_default(),
        ou_id,
        member_ids: vec![],
        endpoint_ids: vec![],
        created_at: Utc::now(),
    };
    s.db.insert_group(&g)?;
    if let Some(m) = inp.member_ids {
        s.db.set_group_members(g.id, &m)?;
    }
    s.db.audit(admin.name(), "group.create", &g.name, "");
    Ok((StatusCode::CREATED, Json(s.db.get_group(g.id)?.expect("just inserted"))))
}

pub async fn update_group(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<GroupInput>,
) -> ApiResult<Json<Group>> {
    let mut g = s.db.get_group(id)?.ok_or_else(|| ApiError::not_found("Group"))?;
    if let Some(n) = inp.name {
        let n = required("Name", &n)?;
        if n != g.name && BUILTIN_GROUPS.contains(&g.name.as_str()) {
            return Err(ApiError::conflict("Built-in groups can't be renamed"));
        }
        g.name = n;
    }
    if let Some(d) = inp.description {
        g.description = d;
    }
    if let Some(o) = inp.ou_id {
        check_ou(&s, o)?;
        g.ou_id = o;
    }
    if let Some(m) = &inp.member_ids {
        if g.name == DOMAIN_ADMINS {
            apply_domain_admin_members(&s, m)?;
        }
    }
    s.db.update_group(&g)?;
    if let Some(m) = inp.member_ids {
        s.db.set_group_members(id, &m)?;
    }
    s.db.audit(admin.name(), "group.update", &g.name, "");
    Ok(Json(s.db.get_group(id)?.expect("exists")))
}

pub async fn set_group_members(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<IdList>,
) -> ApiResult<Json<Group>> {
    let g = s.db.get_group(id)?.ok_or_else(|| ApiError::not_found("Group"))?;
    if g.name == DOMAIN_ADMINS {
        apply_domain_admin_members(&s, &inp.ids)?;
    }
    s.db.set_group_members(id, &inp.ids)?;
    s.db.audit(admin.name(), "group.members", &g.name, &format!("{} members", inp.ids.len()));
    Ok(Json(s.db.get_group(id)?.expect("exists")))
}

/// Computer members: which endpoints belong to this group.
pub async fn set_group_endpoints(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<IdList>,
) -> ApiResult<Json<Group>> {
    let g = s.db.get_group(id)?.ok_or_else(|| ApiError::not_found("Group"))?;
    for e in &inp.ids {
        s.db.get_endpoint(*e)?.ok_or_else(|| ApiError::bad_request("An endpoint in the list no longer exists"))?;
    }
    s.db.set_group_endpoints(id, &inp.ids)?;
    s.db.audit(admin.name(), "group.computers", &g.name, &format!("{} computers", inp.ids.len()));
    Ok(Json(s.db.get_group(id)?.expect("exists")))
}

pub async fn delete_group(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let g = s.db.get_group(id)?.ok_or_else(|| ApiError::not_found("Group"))?;
    if BUILTIN_GROUPS.contains(&g.name.as_str()) {
        return Err(ApiError::conflict("Built-in groups can't be deleted"));
    }
    let used_by: Vec<String> =
        s.db.list_policies()?
            .into_iter()
            .filter(|p| p.settings.iter().any(|st| matches!(st, Setting::LocalAccounts { group_id, .. } if *group_id == id)))
            .map(|p| p.name)
            .collect();
    if !used_by.is_empty() {
        return Err(ApiError::conflict(format!("Used for sign-in accounts by: {}", used_by.join(", "))));
    }
    super::recycle::keep(&s, super::recycle::Snapshot::Group { group: g.clone() }, admin.name())?;
    s.db.delete_group(id)?;
    s.db.audit(admin.name(), "group.delete", &g.name, "Moved to the Recycle Bin");
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
