//! Server maintenance: "Reset settings to defaults" and the authorization
//! step of a factory reset.
//!
//! Both need the signed-in Domain Admin to enter their password again and to
//! type the domain name. A settings reset keeps the directory (users, groups,
//! OUs, computers, GPOs, scripts, certificates); a factory reset erases the
//! whole data directory and is carried out on the server host itself
//! (`opendirectory-server factory-reset`, or the Admin Center after the OS
//! administrator prompt), never over the network.

use axum::extract::State;
use axum::Json;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::Admin;
use crate::crypto::{random_token, sha256_hex, verify_password};
use crate::error::{ApiError, ApiResult};
use crate::AppState;

/// Settings kept in the `meta` table that a settings reset clears, with
/// what each one is, for the confirmation screen.
pub const SETTINGS_KEYS: [(&str, &str); 10] = [
    ("password_policy", "Default Domain Password Policy"),
    ("branding", "Branding"),
    ("audit_retention_days", "Audit log retention"),
    ("recycle_bin_days", "Recycle Bin retention"),
    ("naming_convention", "Computer naming convention"),
    ("agent_updates", "Endpoint Agent update rollout"),
    ("dns_config", "DNS server settings"),
    ("dhcp_config", "DHCP server settings"),
    ("radius_config", "RADIUS server settings"),
    ("ldap_config", "LDAP server settings"),
];
/// Whole tables of settings that a settings reset empties.
pub const SETTINGS_TABLES: [(&str, &str); 2] = [("backup_plans", "Backup plans"), ("integrations", "Integrations")];

const TICKET_KEY: &str = "factory_reset_ticket";

#[derive(Deserialize)]
pub struct Confirm {
    /// The signed-in admin's password, entered again.
    password: String,
    /// Must be the domain name, typed out.
    confirm: String,
}

/// Re-authenticate the admin and check the typed confirmation.
fn confirm(s: &AppState, admin: &Admin, c: &Confirm) -> ApiResult<String> {
    let hash = s.db.user_secrets(admin.user.id)?.password_hash.unwrap_or_default();
    if !verify_password(&c.password, &hash) {
        s.db.audit(admin.name(), "auth.failed", "", "Wrong password when confirming a reset");
        return Err(ApiError::new(axum::http::StatusCode::FORBIDDEN, "That password isn't right"));
    }
    let domain = s.db.meta_get("domain")?.unwrap_or_default();
    if c.confirm.trim().to_lowercase() != domain {
        return Err(ApiError::bad_request(format!("Type the domain name, {domain}, to confirm")));
    }
    Ok(domain)
}

#[derive(Serialize)]
pub struct ResetScope {
    domain: String,
    reset: Vec<&'static str>,
}

/// What a settings reset would clear, for the confirmation screen.
pub async fn settings_scope(State(s): State<AppState>, _a: Admin) -> ApiResult<Json<ResetScope>> {
    let reset = SETTINGS_KEYS.iter().chain(SETTINGS_TABLES.iter()).map(|(_, label)| *label).collect();
    Ok(Json(ResetScope { domain: s.db.meta_get("domain")?.unwrap_or_default(), reset }))
}

/// "Reset settings to defaults": clear configuration, keep the directory.
pub async fn reset_settings(State(s): State<AppState>, admin: Admin, Json(c): Json<Confirm>) -> ApiResult<Json<ResetScope>> {
    let domain = confirm(&s, &admin, &c)?;
    let labels: Vec<&str> = SETTINGS_KEYS.iter().chain(SETTINGS_TABLES.iter()).map(|(_, l)| *l).collect();
    s.db.audit(admin.name(), "settings.reset_defaults", &domain, &format!("Reset to defaults: {}", labels.join(", ")));
    {
        let conn = s.db.lock();
        for (key, _) in SETTINGS_KEYS {
            conn.execute("DELETE FROM meta WHERE key = ?", [key]).map_err(anyhow::Error::from)?;
        }
        for (table, _) in SETTINGS_TABLES {
            conn.execute(&format!("DELETE FROM {table}"), []).map_err(anyhow::Error::from)?;
        }
    }
    // DNS, DHCP, RADIUS and LDAP go back to their defaults (off).
    crate::netsvc::start_all(&s);
    Ok(Json(ResetScope { domain, reset: labels }))
}

#[derive(Serialize)]
pub struct Ticket {
    ticket: String,
    expires_at: chrono::DateTime<Utc>,
}

/// First step of a factory reset: re-authenticate, audit, and issue a
/// single-use ticket valid for five minutes. The erase itself happens on the
/// server host (see `crate::recovery::factory_reset`).
pub async fn authorize_factory_reset(State(s): State<AppState>, admin: Admin, Json(c): Json<Confirm>) -> ApiResult<Json<Ticket>> {
    let domain = confirm(&s, &admin, &c)?;
    let ticket = random_token(24);
    let expires_at = Utc::now() + Duration::minutes(5);
    s.db.meta_set(TICKET_KEY, &format!("{}|{}", sha256_hex(ticket.as_bytes()), expires_at.to_rfc3339()))?;
    s.db.audit(
        admin.name(),
        "domain.factory_reset_requested",
        &domain,
        "Factory reset authorized; it erases the directory once an administrator of the server host approves",
    );
    Ok(Json(Ticket { ticket, expires_at }))
}

/// Check and consume a factory reset ticket.
pub fn take_factory_reset_ticket(db: &crate::db::Db, ticket: &str) -> bool {
    let Ok(Some(v)) = db.meta_get(TICKET_KEY) else { return false };
    let _ = db.lock().execute("DELETE FROM meta WHERE key = ?", [TICKET_KEY]);
    let Some((hash, exp)) = v.split_once('|') else { return false };
    let fresh = chrono::DateTime::parse_from_rfc3339(exp).map(|t| t > Utc::now()).unwrap_or(false);
    fresh && crate::crypto::ct_eq(hash, &sha256_hex(ticket.as_bytes()))
}
