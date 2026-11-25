//! The management API. Every feature in the Admin Center and CLI goes
//! through these routes; nothing is UI-only.

mod agent;
pub mod backups;
mod branding;
mod directory;
mod endpoints;
pub mod integrations;
mod lab;
mod netsvc;
pub mod passwords;
mod policies;
pub mod recycle;
mod scripts;
pub mod shell;

use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(directory::health))
        .route("/setup", get(directory::setup_status).post(directory::setup))
        .route("/branding", get(branding::get).put(branding::set))
        .route("/baselines", get(directory::baselines))
        .route("/baselines/preview", post(directory::preview_baseline))
        .route("/auth/login", post(directory::login))
        .route("/auth/logout", post(directory::logout))
        .route("/auth/change-password", post(passwords::self_service_change))
        .route("/password-policy", get(passwords::get_domain).put(passwords::set_domain))
        .route("/password-policies", get(passwords::list_psos).post(passwords::create_pso))
        .route("/password-policies/{id}", put(passwords::update_pso).delete(passwords::delete_pso))
        .route("/users/{id}/resultant-password-policy", get(passwords::user_resultant))
        .route("/users/{id}/reset-code", post(passwords::issue_reset_code))
        .route("/auth/me", get(directory::me))
        .route("/overview", get(directory::overview))
        .route("/search", get(directory::search))
        .route("/audit", get(directory::audit))
        .route("/ous", get(directory::list_ous).post(directory::create_ou))
        .route("/ous/{id}", get(directory::get_ou).patch(directory::update_ou).delete(directory::delete_ou))
        .route("/users", get(directory::list_users).post(directory::create_user))
        .route("/users/{id}", get(directory::get_user).patch(directory::update_user).delete(directory::delete_user))
        .route("/users/{id}/password", post(directory::reset_password))
        .route("/users/{id}/groups", put(directory::set_user_groups))
        .route("/groups", get(directory::list_groups).post(directory::create_group))
        .route("/groups/{id}", get(directory::get_group).patch(directory::update_group).delete(directory::delete_group))
        .route("/groups/{id}/members", put(directory::set_group_members))
        .route("/groups/{id}/endpoints", put(directory::set_group_endpoints))
        .route("/scripts", get(scripts::list).post(scripts::create))
        .route("/scripts/templates", get(scripts::list_templates))
        .route("/scripts/{id}", get(scripts::get).put(scripts::update).delete(scripts::remove))
        .route("/scripts/{id}/run", post(scripts::run))
        .route("/scripts/{id}/jobs", get(scripts::script_jobs))
        .route("/scripts/{id}/usage", get(scripts::usage))
        .route("/batches/{id}/jobs", get(scripts::batch_jobs))
        .route("/endpoints", get(endpoints::list))
        .route("/endpoints/{id}", get(endpoints::get).patch(endpoints::update).delete(endpoints::remove))
        .route("/endpoints/{id}/jobs", get(endpoints::list_endpoint_jobs).post(endpoints::create_job))
        .route("/endpoints/{id}/rsop", get(policies::rsop))
        .route("/endpoints/{id}/laps", get(endpoints::laps))
        .route("/endpoints/{id}/shell", post(shell::open))
        .route("/shell/{id}/ws", get(shell::admin_ws))
        .route("/endpoints/{id}/laps/rotate", post(endpoints::laps_rotate))
        .route("/jobs", get(endpoints::list_jobs))
        .route("/patches/run", post(endpoints::patch_run))
        .route("/jobs/{id}", get(endpoints::get_job))
        .route("/jobs/{id}/cancel", post(endpoints::cancel_job))
        .route("/enrollment-tokens", get(endpoints::list_tokens).post(endpoints::create_token))
        .route("/enrollment-tokens/{token}", delete(endpoints::delete_token))
        .route("/policies", get(policies::list).post(policies::create))
        .route("/policies/catalog", get(policies::catalog))
        .route("/policies/{id}", get(policies::get).put(policies::update).delete(policies::remove))
        .route("/backups/plans", get(backups::list).post(backups::create))
        .route("/backups/plans/{id}", put(backups::update).delete(backups::remove))
        .route("/backups/plans/{id}/run", post(backups::run_now))
        .route("/backups/plans/{id}/test", post(backups::test_plan))
        .route("/backups/plans/{id}/files", get(backups::files))
        .route("/backups/plans/{id}/restore", post(backups::restore_from_plan))
        .route("/recycle-bin", get(recycle::list))
        .route("/recycle-bin/{id}/restore", post(recycle::restore))
        .route("/recycle-bin/{id}", delete(recycle::purge))
        .route("/dns", get(netsvc::get_dns))
        .route("/dns/config", put(netsvc::set_dns_config))
        .route("/dns/records", put(netsvc::set_dns_records))
        .route("/dhcp", get(netsvc::get_dhcp).put(netsvc::set_dhcp))
        .route("/lab/designs", get(lab::get).put(lab::set))
        .route("/integrations", get(integrations::list).post(integrations::create))
        .route("/integrations/{id}", put(integrations::update).delete(integrations::remove))
        .route("/integrations/{id}/test", post(integrations::test))
        .route("/integrations/{id}/import", post(integrations::import))
        .route("/backups/test", post(backups::test_new))
        .route("/backups/download", post(backups::download_now))
        .route("/backups/restore", post(backups::restore_upload).layer(axum::extract::DefaultBodyLimit::max(256 * 1024 * 1024)))
        .route("/agent/enroll", post(agent::enroll))
        .route("/agent/checkin", post(agent::checkin))
        .route("/agent/jobs/{id}/result", post(agent::job_result))
        .route("/agent/script-runs", post(agent::script_run))
        .route("/agent/laps", post(agent::laps_escrow))
        .route("/agent/wait", get(shell::wait))
        .route("/agent/shell/{id}", get(shell::agent_ws));

    let mut router = Router::new().nest("/api/v1", api);
    // Only for running the Admin Center UI in a browser during development.
    if std::env::var("OD_DEV_CORS").as_deref() == Ok("1") {
        router = router.layer(tower_http::cors::CorsLayer::permissive());
    }
    router.layer(TraceLayer::new_for_http()).layer(axum::extract::DefaultBodyLimit::max(4 * 1024 * 1024)).with_state(state)
}

/// Trim and require a non-empty string field.
pub(crate) fn required(field: &str, value: &str) -> ApiResult<String> {
    let v = value.trim();
    if v.is_empty() {
        return Err(ApiError::bad_request(format!("{field} is required")));
    }
    Ok(v.to_string())
}

/// Account names that must never be created or managed on endpoints.
const RESERVED_ACCOUNTS: &[&str] = &[
    "root",
    "administrator",
    "admin",
    "guest",
    "system",
    "daemon",
    "bin",
    "sys",
    "nobody",
    "defaultaccount",
    "wdagutilityaccount",
    "localsystem",
    "networkservice",
    "localservice",
    "trustedinstaller",
    "krbtgt",
    "wheel",
    "sudo",
    "staff",
    "everyone",
    "users",
    "administrators",
];

/// sAMAccountName-style usernames that are also valid local account names on
/// Windows, macOS and Linux.
pub(crate) fn validate_username(name: &str) -> ApiResult<String> {
    let n = name.trim().to_lowercase();
    let ok_chars = n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-' || c == '_');
    if n.is_empty() || n.len() > 20 || !ok_chars || !n.starts_with(|c: char| c.is_ascii_lowercase()) {
        return Err(ApiError::bad_request(
            "Usernames are 1 to 20 characters, start with a letter, and use only letters, numbers, dots, dashes or underscores",
        ));
    }
    if RESERVED_ACCOUNTS.contains(&n.as_str()) {
        return Err(ApiError::bad_request(format!("\"{n}\" is reserved by operating systems; choose another username")));
    }
    Ok(n)
}

pub(crate) fn validate_password(pw: &str) -> ApiResult<()> {
    if pw.chars().count() < 8 {
        return Err(ApiError::bad_request("Passwords must be at least 8 characters"));
    }
    if pw.chars().count() > 256 {
        return Err(ApiError::bad_request("Passwords must be at most 256 characters"));
    }
    if pw.contains(['\n', '\r', '\0']) {
        return Err(ApiError::bad_request("Passwords can't contain line breaks"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
