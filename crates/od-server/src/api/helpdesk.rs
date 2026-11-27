//! Help desk: chat between the person at a device and IT. Users write from
//! the agent's local "Contact IT" page (or the Android app); admins answer
//! from the Admin Center. Messages reach the device at its next check-in,
//! which a reply triggers right away.

use axum::extract::{Path, State};
use axum::Json;
use od_core::policy::Setting;
use od_core::protocol::{ChatMessage, ChatPost};
use serde::Serialize;
use uuid::Uuid;

use crate::auth::{Admin, EndpointCaller};
use crate::error::{ApiError, ApiResult};
use crate::AppState;

const MAX_LEN: usize = 4000;

fn clean(text: &str) -> ApiResult<String> {
    let t = text.trim();
    if t.is_empty() {
        return Err(ApiError::bad_request("The message is empty"));
    }
    if t.chars().count() > MAX_LEN {
        return Err(ApiError::bad_request(format!("Messages are limited to {MAX_LEN} characters")));
    }
    Ok(t.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect())
}

/// The device's user sends a message to IT.
pub async fn agent_post(
    State(s): State<AppState>,
    EndpointCaller(auth): EndpointCaller,
    Json(p): Json<ChatPost>,
) -> ApiResult<Json<ChatMessage>> {
    let e = s.db.get_endpoint(auth.id)?.ok_or_else(|| ApiError::not_found("Endpoint"))?;
    if !super::policies::endpoint_settings(&s, &e)?.iter().any(|st| matches!(st, Setting::HelpDesk { enabled: true, .. })) {
        return Err(ApiError::forbidden("Help desk chat isn't turned on for this device"));
    }
    let text = clean(&p.text)?;
    let author: String = p.author.trim().chars().filter(|c| !c.is_control()).take(64).collect();
    let author = if author.is_empty() { "Device user".to_string() } else { author };
    let m = s.db.add_chat(auth.id, false, &author, &text)?;
    // The audit entry (and any webhook) says a message arrived, not what it says.
    s.db.audit(&author, "helpdesk.message", &e.hostname, "New help desk message from the device's user");
    Ok(Json(m))
}

#[derive(Serialize)]
pub struct Thread {
    endpoint_id: Uuid,
    hostname: String,
    last: ChatMessage,
    unread: i64,
}

pub async fn threads(State(s): State<AppState>, _a: Admin) -> ApiResult<Json<Vec<Thread>>> {
    let names: std::collections::HashMap<Uuid, String> = s.db.list_endpoints()?.into_iter().map(|e| (e.id, e.hostname)).collect();
    Ok(Json(
        s.db.chat_threads()?
            .into_iter()
            .map(|(id, last, unread)| Thread { hostname: names.get(&id).cloned().unwrap_or_default(), endpoint_id: id, last, unread })
            .collect(),
    ))
}

/// The whole conversation with one device. Reading it marks it read.
pub async fn messages(State(s): State<AppState>, _a: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<ChatMessage>>> {
    s.db.get_endpoint(id)?.ok_or_else(|| ApiError::not_found("Endpoint"))?;
    let v = s.db.chat_after(id, 0, 500)?;
    if !s.replication.is_replica() {
        s.db.mark_chat_read(id)?;
    }
    Ok(Json(v))
}

#[derive(serde::Deserialize)]
pub struct Reply {
    text: String,
}

pub async fn reply(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>, Json(r): Json<Reply>) -> ApiResult<Json<ChatMessage>> {
    let e = s.db.get_endpoint(id)?.ok_or_else(|| ApiError::not_found("Endpoint"))?;
    let text = clean(&r.text)?;
    let who = if admin.user.display_name.trim().is_empty() { admin.name().to_string() } else { admin.user.display_name.clone() };
    let m = s.db.add_chat(id, true, &who, &text)?;
    s.db.mark_chat_read(id)?;
    s.hub.wake(id);
    s.db.audit(admin.name(), "helpdesk.reply", &e.hostname, "Replied in help desk chat");
    Ok(Json(m))
}
