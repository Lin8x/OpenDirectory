//! Network Lab designs, stored on the Management Server so every admin sees them.

use axum::extract::State;
use axum::Json;
use serde_json::Value;

use crate::auth::Admin;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub async fn get(State(s): State<AppState>, _a: Admin) -> ApiResult<Json<Value>> {
    Ok(Json(s.db.meta_get("lab_designs")?.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or(Value::Array(vec![]))))
}

pub async fn set(State(s): State<AppState>, admin: Admin, Json(v): Json<Value>) -> ApiResult<Json<Value>> {
    let arr = v.as_array().ok_or_else(|| ApiError::bad_request("Expected a list of designs"))?;
    if arr.len() > 200 {
        return Err(ApiError::bad_request("At most 200 designs"));
    }
    let raw = serde_json::to_string(&v).map_err(anyhow::Error::from)?;
    if raw.len() > 3 * 1024 * 1024 {
        return Err(ApiError::bad_request("Designs are limited to 3 MB in total"));
    }
    s.db.meta_set("lab_designs", &raw)?;
    s.db.audit(admin.name(), "lab.save", "", &format!("{} design(s)", arr.len()));
    Ok(Json(v))
}
