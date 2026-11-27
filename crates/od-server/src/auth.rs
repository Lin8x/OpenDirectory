//! Request authentication for administrators and Endpoint Agents.

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use od_core::{Role, User};
use uuid::Uuid;

use crate::crypto::{ct_eq, sha256_hex};
use crate::db::EndpointAuth;
use crate::error::ApiError;
use crate::AppState;

/// A signed-in administrator. Only users with the Admin role and an enabled
/// account can use the management API.
pub struct Admin {
    pub user: User,
    pub token_hash: String,
}

impl Admin {
    pub fn name(&self) -> &str {
        &self.user.username
    }
}

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts.headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).ok_or_else(ApiError::unauthorized)?;
        let token = header.strip_prefix("Bearer ").ok_or_else(ApiError::unauthorized)?.trim();
        let token_hash = sha256_hex(token.as_bytes());
        let user_id = state.db.session_user(&token_hash)?.ok_or_else(ApiError::unauthorized)?;
        let user = state.db.get_user(user_id)?.ok_or_else(ApiError::unauthorized)?;
        if !user.enabled || user.role != Role::Admin {
            return Err(ApiError::forbidden("This account can't use the Admin Center"));
        }
        mfa_gate(parts, state, &user, true)?;
        Ok(Admin { user, token_hash })
    }
}

/// Any signed-in Admin Center account: a Domain Admin, or a delegated
/// operator with at least one active delegation. Handlers that accept an
/// `Operator` must check `access` for every object they return or touch.
pub struct Operator {
    pub user: User,
    pub token_hash: String,
    pub access: crate::rbac::Access,
}

impl Operator {
    pub fn name(&self) -> &str {
        &self.user.username
    }
    /// Refuse unless this operator holds the permission on the endpoint.
    /// Endpoints outside the scope look like they don't exist.
    pub fn need_endpoint(&self, e: &od_core::Endpoint, p: crate::rbac::Permission) -> Result<(), ApiError> {
        if self.access.endpoint(e, p) {
            Ok(())
        } else if self.access.sees_endpoint(e) {
            Err(ApiError::forbidden("Your delegation doesn't include that for this endpoint"))
        } else {
            Err(ApiError::not_found("Endpoint"))
        }
    }
    pub fn need_user(&self, u: &User, p: crate::rbac::Permission) -> Result<(), ApiError> {
        if self.access.user(u, p) {
            Ok(())
        } else if self.access.sees_user(u) {
            Err(ApiError::forbidden("Your delegation doesn't include that for this account"))
        } else {
            Err(ApiError::not_found("User"))
        }
    }
}

impl FromRequestParts<AppState> for Operator {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts.headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).ok_or_else(ApiError::unauthorized)?;
        let token = header.strip_prefix("Bearer ").ok_or_else(ApiError::unauthorized)?.trim();
        let token_hash = sha256_hex(token.as_bytes());
        let user_id = state.db.session_user(&token_hash)?.ok_or_else(ApiError::unauthorized)?;
        let user = state.db.get_user(user_id)?.ok_or_else(ApiError::unauthorized)?;
        if !user.enabled {
            return Err(ApiError::forbidden("This account can't use the Admin Center"));
        }
        let access = crate::rbac::Access::resolve(state, &user)?;
        if access.is_empty() {
            // Delegation removed or expired: end the session.
            let _ = state.db.delete_session(&token_hash);
            return Err(ApiError::new(axum::http::StatusCode::UNAUTHORIZED, "Your delegated access has ended"));
        }
        mfa_gate(parts, state, &user, access.domain_admin)?;
        Ok(Operator { user, token_hash, access })
    }
}

/// When the domain requires MFA and this account hasn't set it up, allow
/// only what's needed to enroll (and to sign out).
fn mfa_gate(parts: &Parts, state: &AppState, user: &User, domain_admin: bool) -> Result<(), ApiError> {
    let path = parts.uri.path();
    let allowed = ["/auth/me", "/auth/logout", "/auth/mfa/setup", "/auth/mfa/enable", "/branding"].iter().any(|p| path.ends_with(p));
    if !allowed && crate::api::mfa::must_enroll(state, user.id, domain_admin) {
        return Err(ApiError::forbidden("Set up two-factor sign-in to continue").with_code("mfa_enrollment_required"));
    }
    Ok(())
}

/// An enrolled Endpoint Agent, authenticated with `Authorization: Endpoint <id>.<secret>`.
pub struct EndpointCaller(pub EndpointAuth);

impl FromRequestParts<AppState> for EndpointCaller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts.headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).ok_or_else(ApiError::unauthorized)?;
        let cred = header.strip_prefix("Endpoint ").ok_or_else(ApiError::unauthorized)?.trim();
        let (id, secret) = cred.split_once('.').ok_or_else(ApiError::unauthorized)?;
        let id = Uuid::parse_str(id).map_err(|_| ApiError::unauthorized())?;
        let auth = state.db.endpoint_auth(id)?.ok_or_else(ApiError::unauthorized)?;
        if !ct_eq(&auth.secret_hash, &sha256_hex(secret.as_bytes())) {
            return Err(ApiError::unauthorized());
        }
        Ok(EndpointCaller(auth))
    }
}
