//! Two-factor sign-in (MFA) with time-based one-time passwords (TOTP,
//! RFC 6238): the six-digit codes from Microsoft Authenticator, Google
//! Authenticator, 1Password, Bitwarden, Aegis and similar apps.
//!
//! Secrets are sealed with the server's master key. A domain-wide policy can
//! require MFA for delegated operators or for every Admin Center account;
//! accounts without it are then held at the enrollment screen.

use std::collections::HashMap;
use std::sync::Mutex;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::{Admin, Operator};
use crate::crypto::verify_password;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

const STEP: u64 = 30;
const DIGITS: u32 = 6;

/// Who must use MFA to sign in to the Admin Center.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MfaRequirement {
    /// Optional for everyone.
    #[default]
    Optional,
    /// Required for delegated operators (not Domain Admins).
    Delegated,
    /// Required for every account, Domain Admins included.
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MfaPolicy {
    #[serde(default)]
    pub require: MfaRequirement,
}

pub fn policy(s: &AppState) -> MfaPolicy {
    s.db.meta_get("mfa_policy").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}

pub fn enabled(s: &AppState, user: Uuid) -> bool {
    s.db.mfa_secrets(user).map(|(a, _)| a.is_some()).unwrap_or(false)
}

/// Whether this account must enroll before using the console.
pub fn must_enroll(s: &AppState, user: Uuid, domain_admin: bool) -> bool {
    let required = match policy(s).require {
        MfaRequirement::Optional => false,
        MfaRequirement::Delegated => !domain_admin,
        MfaRequirement::All => true,
    };
    required && !enabled(s, user)
}

// ---------------------------------------------------------------- TOTP

fn hotp(secret: &[u8], counter: u64) -> u32 {
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(secret).expect("any key length");
    mac.update(&counter.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let off = (h[19] & 0x0f) as usize;
    let bin = u32::from_be_bytes([h[off] & 0x7f, h[off + 1], h[off + 2], h[off + 3]]);
    bin % 10u32.pow(DIGITS)
}

fn now_step() -> u64 {
    (chrono::Utc::now().timestamp().max(0) as u64) / STEP
}

/// The time step a code matches, allowing one step of clock drift each way.
fn check_code(secret: &[u8], code: &str, step: u64) -> Option<u64> {
    let code: String = code.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    if code.len() != DIGITS as usize || !code.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let want: u32 = code.parse().ok()?;
    [step.saturating_sub(1), step, step + 1]
        .into_iter()
        .find(|t| crate::crypto::ct_eq(&format!("{:06}", hotp(secret, *t)), &format!("{want:06}")))
}

/// Last accepted step per account, so a code can't be replayed.
static LAST_STEP: Mutex<Option<HashMap<Uuid, u64>>> = Mutex::new(None);

/// Verify a code against the account's active secret. Each code works once.
pub fn verify(s: &AppState, user: Uuid, code: &str) -> ApiResult<bool> {
    let Some(sealed) = s.db.mfa_secrets(user)?.0 else { return Ok(false) };
    let secret = s.sealer.open(&sealed)?;
    let Some(step) = check_code(&secret, code, now_step()) else { return Ok(false) };
    let mut g = LAST_STEP.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(HashMap::new);
    if m.get(&user).is_some_and(|last| step <= *last) {
        return Ok(false);
    }
    m.insert(user, step);
    Ok(true)
}

const B32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32(data: &[u8]) -> String {
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for b in data {
        buf = (buf << 8) | *b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32[((buf >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn uri_escape(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

// ---------------------------------------------------------------- API

#[derive(Serialize)]
pub struct Setup {
    /// Base32 secret to type into an authenticator app.
    secret: String,
    /// `otpauth://` link, shown as a QR code by the Admin Center.
    otpauth_uri: String,
}

/// Start enrolling: make a new secret. It becomes active once a code from
/// it is confirmed with `enable`.
pub async fn setup(State(s): State<AppState>, op: Operator) -> ApiResult<Json<Setup>> {
    let mut secret = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut secret);
    s.db.set_mfa_pending(op.user.id, Some(&s.sealer.seal(&secret)?))?;
    let domain = s.db.meta_get("domain")?.unwrap_or_default();
    let issuer = format!("OpenDirectory {domain}").trim().to_string();
    let label = format!("{}:{}@{}", issuer, op.user.username, domain);
    let b32 = base32(&secret);
    let otpauth_uri = format!(
        "otpauth://totp/{}?secret={b32}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={STEP}",
        uri_escape(&label),
        uri_escape(&issuer)
    );
    Ok(Json(Setup { secret: b32, otpauth_uri }))
}

#[derive(Deserialize)]
pub struct CodeInput {
    code: String,
}

pub async fn enable(State(s): State<AppState>, op: Operator, Json(inp): Json<CodeInput>) -> ApiResult<Json<Value>> {
    let pending = s.db.mfa_secrets(op.user.id)?.1.ok_or_else(|| ApiError::bad_request("Start MFA setup first"))?;
    let secret = s.sealer.open(&pending)?;
    let Some(step) = check_code(&secret, &inp.code, now_step()) else {
        return Err(ApiError::bad_request("That code isn't right. Check the time on your phone and try the newest code."));
    };
    s.db.set_mfa_secret(op.user.id, Some(&pending))?;
    LAST_STEP.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(op.user.id, step);
    s.db.audit(op.name(), "mfa.enable", op.name(), "Authenticator app registered");
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct DisableInput {
    password: String,
    code: String,
}

/// Turn off your own MFA. Needs your password and a current code.
pub async fn disable(State(s): State<AppState>, op: Operator, Json(inp): Json<DisableInput>) -> ApiResult<StatusCode> {
    if must_enroll_if_removed(&s, &op) {
        return Err(ApiError::forbidden("MFA is required for your account by the domain's sign-in policy"));
    }
    let hash = s.db.user_secrets(op.user.id)?.password_hash.unwrap_or_default();
    if !verify_password(&inp.password, &hash) || !verify(&s, op.user.id, &inp.code)? {
        return Err(ApiError::bad_request("The password or code is incorrect"));
    }
    s.db.set_mfa_secret(op.user.id, None)?;
    s.db.audit(op.name(), "mfa.disable", op.name(), "");
    Ok(StatusCode::NO_CONTENT)
}

fn must_enroll_if_removed(s: &AppState, op: &Operator) -> bool {
    match policy(s).require {
        MfaRequirement::Optional => false,
        MfaRequirement::Delegated => !op.access.domain_admin,
        MfaRequirement::All => true,
    }
}

/// A Domain Admin resets someone's MFA (lost phone). They enroll again at
/// their next sign-in if the policy requires it.
pub async fn reset_for_user(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let u = s.db.get_user(id)?.ok_or_else(|| ApiError::not_found("User"))?;
    s.db.set_mfa_secret(id, None)?;
    if u.id != admin.user.id {
        s.db.delete_user_sessions(id)?;
    }
    s.db.audit(admin.name(), "mfa.reset", &u.username, "Authenticator removed; signed out");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_policy(State(s): State<AppState>, _admin: Admin) -> ApiResult<Json<MfaPolicy>> {
    Ok(Json(policy(&s)))
}

pub async fn set_policy(State(s): State<AppState>, admin: Admin, Json(p): Json<MfaPolicy>) -> ApiResult<Json<MfaPolicy>> {
    s.db.meta_set("mfa_policy", &serde_json::to_string(&p).map_err(anyhow::Error::from)?)?;
    let label = match p.require {
        MfaRequirement::Optional => "Optional",
        MfaRequirement::Delegated => "Required for delegated operators",
        MfaRequirement::All => "Required for every account",
    };
    s.db.audit(admin.name(), "mfa.policy", "", label);
    Ok(Json(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_sha1_vectors() {
        // RFC 6238 appendix B, truncated to 6 digits.
        let key = b"12345678901234567890";
        assert_eq!(hotp(key, 59 / 30), 287082);
        assert_eq!(hotp(key, 1111111109 / 30), 81804);
        assert_eq!(hotp(key, 1234567890 / 30), 5924);
        assert_eq!(hotp(key, 2000000000 / 30), 279037);
    }

    #[test]
    fn drift_window_and_format() {
        let key = b"12345678901234567890";
        let step = 1111111109 / 30;
        assert_eq!(check_code(key, "081804", step), Some(step));
        assert_eq!(check_code(key, "081 804", step + 1), Some(step));
        assert_eq!(check_code(key, "081804", step + 2), None);
        assert_eq!(check_code(key, "81804", step), None);
    }

    #[test]
    fn base32_rfc4648() {
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32(b"12345678901234567890"), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
    }
}
