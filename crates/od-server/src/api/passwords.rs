//! Directory password rules: the Default Domain Password Policy, fine-grained
//! password policies (PSOs) applied to security groups by precedence, password
//! history and expiry, self-service password change, and help-desk reset codes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::required;
use crate::auth::Admin;
use crate::crypto::{hash_password, random_token, sha256_hex, verify_password};
use crate::db::Db;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Rules {
    #[serde(default = "eight")]
    pub min_length: u32,
    /// At least three of: upper-case, lower-case, digit, symbol.
    #[serde(default)]
    pub complexity: bool,
    /// Remember this many previous passwords.
    #[serde(default)]
    pub history: u32,
    /// Days until a password expires (0 = never).
    #[serde(default)]
    pub max_age_days: u32,
    #[serde(default = "five")]
    pub lockout_threshold: u32,
    #[serde(default = "five")]
    pub lockout_minutes: u32,
}
fn eight() -> u32 {
    8
}
fn five() -> u32 {
    5
}
impl Default for Rules {
    fn default() -> Self {
        Rules { min_length: 8, complexity: false, history: 0, max_age_days: 0, lockout_threshold: 5, lockout_minutes: 5 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pso {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    /// Lower wins when a user is in several groups with PSOs (like msDS-PasswordSettingsPrecedence).
    pub precedence: i32,
    pub group_ids: Vec<Uuid>,
    pub rules: Rules,
}

pub fn domain_rules(s: &AppState) -> Rules {
    domain_rules_db(&s.db)
}

fn domain_rules_db(db: &Db) -> Rules {
    db.meta_get("password_policy").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}

fn psos(s: &AppState) -> Vec<Pso> {
    psos_db(&s.db)
}

fn psos_db(db: &Db) -> Vec<Pso> {
    let mut v: Vec<Pso> = db.meta_get("password_psos").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    v.sort_by_key(|p| (p.precedence, p.name.to_lowercase()));
    v
}
fn save_psos(s: &AppState, v: &[Pso]) -> ApiResult<()> {
    s.db.meta_set("password_psos", &serde_json::to_string(v).map_err(anyhow::Error::from)?)?;
    Ok(())
}

#[derive(Serialize)]
pub struct Resultant {
    pub rules: Rules,
    /// "Default Domain Password Policy" or the PSO's name.
    pub source: String,
    pub pso_id: Option<Uuid>,
}

/// The rules that apply to a user (Resultant PSO).
pub fn resultant(s: &AppState, user_id: Option<Uuid>) -> Resultant {
    resultant_db(&s.db, user_id)
}

fn resultant_db(db: &Db, user_id: Option<Uuid>) -> Resultant {
    if let Some(uid) = user_id {
        if let Ok(Some(u)) = db.get_user(uid) {
            if let Some(p) = psos_db(db).into_iter().find(|p| p.group_ids.iter().any(|g| u.group_ids.contains(g))) {
                return Resultant { rules: p.rules, source: p.name, pso_id: Some(p.id) };
            }
        }
    }
    Resultant { rules: domain_rules_db(db), source: "Default Domain Password Policy".into(), pso_id: None }
}

fn validate_rules(r: &Rules) -> ApiResult<()> {
    if !(4..=128).contains(&r.min_length) {
        return Err(ApiError::bad_request("Minimum length must be 4 to 128"));
    }
    if r.history > 24 || r.max_age_days > 999 || r.lockout_threshold > 999 || r.lockout_minutes > 99999 {
        return Err(ApiError::bad_request("A value is out of range (history up to 24, age up to 999 days)"));
    }
    Ok(())
}

/// Check a new password against the user's rules and history.
pub fn check(s: &AppState, user_id: Option<Uuid>, pw: &str) -> ApiResult<()> {
    check_db(&s.db, user_id, pw)
}

/// [`check`] against a database opened outside a running server (the local
/// recovery commands).
pub fn check_db(db: &Db, user_id: Option<Uuid>, pw: &str) -> ApiResult<()> {
    super::validate_password(pw)?;
    let r = resultant_db(db, user_id);
    let rules = &r.rules;
    if (pw.chars().count() as u32) < rules.min_length {
        return Err(ApiError::bad_request(format!("Passwords must be at least {} characters ({})", rules.min_length, r.source)));
    }
    if rules.complexity {
        let classes = [
            pw.chars().any(|c| c.is_lowercase()),
            pw.chars().any(|c| c.is_uppercase()),
            pw.chars().any(|c| c.is_ascii_digit()),
            pw.chars().any(|c| !c.is_alphanumeric()),
        ]
        .iter()
        .filter(|x| **x)
        .count();
        if classes < 3 {
            return Err(ApiError::bad_request("Use at least three of: upper-case letters, lower-case letters, numbers and symbols"));
        }
    }
    if rules.history > 0 {
        if let Some(uid) = user_id {
            let mut hashes = db.password_history(uid, rules.history)?;
            if let Some(cur) = db.user_secrets(uid)?.password_hash {
                hashes.insert(0, cur);
            }
            if hashes.iter().take(rules.history as usize).any(|h| verify_password(pw, h)) {
                return Err(ApiError::bad_request(format!("You can't reuse any of your last {} passwords", rules.history)));
            }
        }
    }
    Ok(())
}

/// Store a new password (already checked), keeping the old hash in history.
pub fn set(s: &AppState, user_id: Uuid, pw: &str) -> ApiResult<()> {
    set_db(&s.db, &s.sealer, user_id, pw)
}

/// [`set`] against a database opened outside a running server.
pub fn set_db(db: &Db, sealer: &crate::crypto::Sealer, user_id: Uuid, pw: &str) -> ApiResult<()> {
    if let Some(old) = db.user_secrets(user_id)?.password_hash {
        db.push_password_history(user_id, &old)?;
    }
    db.set_user_password(user_id, &hash_password(pw)?, &sealer.seal(pw.as_bytes())?)?;
    Ok(())
}

/// Has this user's password passed its maximum age?
pub fn expired(s: &AppState, user: &od_core::User) -> bool {
    let r = resultant(s, Some(user.id)).rules;
    r.max_age_days > 0 && user.password_set_at.map(|t| Utc::now() - t > Duration::days(r.max_age_days as i64)).unwrap_or(false)
}

// ---------------------------------------------------------------- admin API

pub async fn get_domain(State(s): State<AppState>, _a: Admin) -> Json<Rules> {
    Json(domain_rules(&s))
}

pub async fn set_domain(State(s): State<AppState>, admin: Admin, Json(r): Json<Rules>) -> ApiResult<Json<Rules>> {
    validate_rules(&r)?;
    s.db.meta_set("password_policy", &serde_json::to_string(&r).map_err(anyhow::Error::from)?)?;
    s.db.audit(admin.name(), "password_policy.update", "Default Domain Password Policy", &format!("min {} chars", r.min_length));
    Ok(Json(r))
}

pub async fn list_psos(State(s): State<AppState>, _a: Admin) -> Json<Vec<Pso>> {
    Json(psos(&s))
}

#[derive(Deserialize)]
pub struct PsoInput {
    name: String,
    #[serde(default)]
    description: String,
    precedence: i32,
    #[serde(default)]
    group_ids: Vec<Uuid>,
    rules: Rules,
}

fn apply(s: &AppState, p: &mut Pso, inp: PsoInput) -> ApiResult<()> {
    p.name = required("Name", &inp.name)?;
    validate_rules(&inp.rules)?;
    for g in &inp.group_ids {
        s.db.get_group(*g)?.ok_or_else(|| ApiError::bad_request("A chosen group no longer exists"))?;
    }
    p.description = inp.description;
    p.precedence = inp.precedence;
    p.group_ids = inp.group_ids;
    p.rules = inp.rules;
    Ok(())
}

pub async fn create_pso(State(s): State<AppState>, admin: Admin, Json(inp): Json<PsoInput>) -> ApiResult<(StatusCode, Json<Pso>)> {
    let mut all = psos(&s);
    let mut p = Pso {
        id: Uuid::new_v4(),
        name: String::new(),
        description: String::new(),
        precedence: 10,
        group_ids: vec![],
        rules: Rules::default(),
    };
    apply(&s, &mut p, inp)?;
    if all.iter().any(|x| x.name.eq_ignore_ascii_case(&p.name)) {
        return Err(ApiError::conflict("A password policy with that name exists"));
    }
    all.push(p.clone());
    save_psos(&s, &all)?;
    s.db.audit(admin.name(), "pso.create", &p.name, &format!("precedence {}", p.precedence));
    Ok((StatusCode::CREATED, Json(p)))
}

pub async fn update_pso(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>, Json(inp): Json<PsoInput>) -> ApiResult<Json<Pso>> {
    let mut all = psos(&s);
    let p = all.iter_mut().find(|p| p.id == id).ok_or_else(|| ApiError::not_found("Password policy"))?;
    apply(&s, p, inp)?;
    let out = p.clone();
    save_psos(&s, &all)?;
    s.db.audit(admin.name(), "pso.update", &out.name, "");
    Ok(Json(out))
}

pub async fn delete_pso(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let mut all = psos(&s);
    let name = all.iter().find(|p| p.id == id).map(|p| p.name.clone()).ok_or_else(|| ApiError::not_found("Password policy"))?;
    all.retain(|p| p.id != id);
    save_psos(&s, &all)?;
    s.db.audit(admin.name(), "pso.delete", &name, "");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn user_resultant(State(s): State<AppState>, _a: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<Resultant>> {
    s.db.get_user(id)?.ok_or_else(|| ApiError::not_found("User"))?;
    Ok(Json(resultant(&s, Some(id))))
}

#[derive(Serialize)]
pub struct ResetCode {
    code: String,
    expires_at: chrono::DateTime<Utc>,
}

/// Help desk: issue a one-time code the user redeems to choose a new password.
pub async fn issue_reset_code(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<ResetCode>> {
    let u = s.db.get_user(id)?.ok_or_else(|| ApiError::not_found("User"))?;
    let raw = random_token(5).to_uppercase();
    let code = format!("{}-{}", &raw[..5], &raw[5..]);
    let expires_at = Utc::now() + Duration::hours(24);
    s.db.set_reset_code(id, &sha256_hex(code.replace('-', "").as_bytes()), expires_at)?;
    s.db.audit(admin.name(), "user.reset_code", &u.username, "Valid for 24 hours");
    Ok(Json(ResetCode { code, expires_at }))
}

// ---------------------------------------------------------------- self-service (public)

#[derive(Deserialize)]
pub struct ChangeInput {
    username: String,
    /// The current password, or a reset code from the help desk.
    #[serde(default)]
    current_password: String,
    #[serde(default)]
    reset_code: String,
    new_password: String,
}

fn normalize(name: &str) -> String {
    let n = name.trim().to_lowercase();
    let n = n.rsplit('\\').next().unwrap_or(&n).to_string();
    n.split('@').next().unwrap_or(&n).to_string()
}

/// Change your own password with the current one or a reset code. Uses the
/// same lockout as sign-in, so it can't be used to guess passwords.
pub async fn self_service_change(State(s): State<AppState>, Json(inp): Json<ChangeInput>) -> ApiResult<Json<serde_json::Value>> {
    let name = normalize(&inp.username);
    let generic = || ApiError::new(StatusCode::UNAUTHORIZED, "The username, password or code is incorrect");
    if super::directory::locked_out(&s, &name).is_some() {
        return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "Too many failed attempts. Try again later."));
    }
    let Some(user) = s.db.get_user_by_username(&name)? else {
        super::directory::record_failed_login(&s, &name, "Self-service change: unknown account");
        return Err(generic());
    };
    let ok = if !inp.reset_code.trim().is_empty() {
        let code = inp.reset_code.trim().to_uppercase().replace(['-', ' '], "");
        s.db.take_reset_code(user.id, &sha256_hex(code.as_bytes()))?
    } else {
        s.db.user_secrets(user.id)?.password_hash.map(|h| verify_password(&inp.current_password, &h)).unwrap_or(false)
    };
    if !ok {
        super::directory::record_failed_login(&s, &name, "Self-service change: wrong password or code");
        return Err(generic());
    }
    if !user.enabled {
        return Err(ApiError::forbidden("This account is disabled. Contact your IT team."));
    }
    check(&s, Some(user.id), &inp.new_password)?;
    set(&s, user.id, &inp.new_password)?;
    let mut u = user.clone();
    u.must_change_password = false;
    s.db.update_user(&u)?;
    s.db.delete_user_sessions(u.id)?;
    s.login_failures.lock().unwrap_or_else(|e| e.into_inner()).remove(&name);
    s.db.audit(
        &u.username,
        "user.password_change",
        &u.username,
        if inp.reset_code.is_empty() { "Self-service" } else { "With a help-desk reset code" },
    );
    Ok(Json(serde_json::json!({ "ok": true, "message": "Password changed. Managed computers pick it up at their next check-in." })))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules_defaults_are_sane() {
        let r: Rules = serde_json::from_str("{}").unwrap();
        assert_eq!(r, Rules::default());
        assert!(validate_rules(&Rules { min_length: 2, ..Default::default() }).is_err());
    }
}
