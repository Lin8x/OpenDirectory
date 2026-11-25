//! The management API. Every feature in the Admin Center and CLI goes
//! through these routes; nothing is UI-only.

mod agent;
mod directory;
mod endpoints;
mod policies;

use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(directory::health))
        .route("/setup", get(directory::setup_status).post(directory::setup))
        .route("/auth/login", post(directory::login))
        .route("/auth/logout", post(directory::logout))
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
        .route("/endpoints", get(endpoints::list))
        .route("/endpoints/{id}", get(endpoints::get).patch(endpoints::update).delete(endpoints::remove))
        .route("/endpoints/{id}/jobs", get(endpoints::list_endpoint_jobs).post(endpoints::create_job))
        .route("/endpoints/{id}/rsop", get(policies::rsop))
        .route("/jobs", get(endpoints::list_jobs))
        .route("/jobs/{id}", get(endpoints::get_job))
        .route("/jobs/{id}/cancel", post(endpoints::cancel_job))
        .route("/enrollment-tokens", get(endpoints::list_tokens).post(endpoints::create_token))
        .route("/enrollment-tokens/{token}", delete(endpoints::delete_token))
        .route("/policies", get(policies::list).post(policies::create))
        .route("/policies/catalog", get(policies::catalog))
        .route("/policies/{id}", get(policies::get).put(policies::update).delete(policies::remove))
        .route("/agent/enroll", post(agent::enroll))
        .route("/agent/checkin", post(agent::checkin))
        .route("/agent/jobs/{id}/result", post(agent::job_result));

    let mut router = Router::new().nest("/api/v1", api);
    // Only for running the Admin Center UI in a browser during development.
    if std::env::var("OD_DEV_CORS").as_deref() == Ok("1") {
        router = router.layer(tower_http::cors::CorsLayer::permissive());
    }
    router
        .layer(TraceLayer::new_for_http())
        .layer(axum::extract::DefaultBodyLimit::max(4 * 1024 * 1024))
        .with_state(state)
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
    "root", "administrator", "admin", "guest", "system", "daemon", "bin", "sys", "nobody", "defaultaccount",
    "wdagutilityaccount", "localsystem", "networkservice", "localservice", "trustedinstaller", "krbtgt", "wheel",
    "sudo", "staff", "everyone", "users", "administrators",
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
