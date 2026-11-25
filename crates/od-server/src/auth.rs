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
        Ok(Admin { user, token_hash })
    }
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
