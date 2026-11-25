//! Remote shell: an interactive terminal on an endpoint, relayed through the
//! Management Server over two WebSockets (admin ↔ server ↔ agent), so
//! endpoints never need an inbound port. Also the agent wake-up long poll that
//! makes jobs and shells start within a second instead of at the next check-in.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Notify};
use uuid::Uuid;

use crate::auth::{Admin, EndpointCaller};
use crate::crypto::{ct_eq, random_token, sha256_hex};
use crate::error::{ApiError, ApiResult};
use crate::AppState;

type Tx = mpsc::UnboundedSender<Message>;

struct Session {
    endpoint: Uuid,
    host: String,
    admin: String,
    ticket_hash: Option<String>,
    created: Instant,
    cols: u16,
    rows: u16,
    /// Set for a TCP tunnel to this port on the endpoint (remote desktop, SSH).
    tunnel_port: Option<u16>,
    delivered: bool,
    to_admin: Option<Tx>,
    to_agent: Option<Tx>,
    /// Data that arrived before the other side connected.
    admin_backlog: Vec<Message>,
    agent_backlog: Vec<Message>,
}

#[derive(Default)]
pub struct Hub {
    sessions: Mutex<HashMap<Uuid, Session>>,
    wakers: Mutex<HashMap<Uuid, Arc<Notify>>>,
}

impl Hub {
    fn waker(&self, endpoint: Uuid) -> Arc<Notify> {
        self.wakers.lock().unwrap_or_else(|e| e.into_inner()).entry(endpoint).or_default().clone()
    }
    /// Tell an endpoint's agent to check in now (a job or shell is waiting).
    pub fn wake(&self, endpoint: Uuid) {
        self.waker(endpoint).notify_one();
    }
    /// Shell sessions the agent should open, handed out once.
    pub fn take_requests(&self, endpoint: Uuid) -> Vec<od_core::protocol::ShellRequest> {
        let mut m = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        m.retain(|_, s| {
            s.created.elapsed() < Duration::from_secs(12 * 3600) && (s.delivered || s.created.elapsed() < Duration::from_secs(120))
        });
        m.iter_mut()
            .filter(|(_, s)| s.endpoint == endpoint && !s.delivered)
            .map(|(id, s)| {
                s.delivered = true;
                od_core::protocol::ShellRequest { session_id: *id, cols: s.cols, rows: s.rows, tunnel_port: s.tunnel_port }
            })
            .collect()
    }
}

#[derive(Deserialize)]
pub struct OpenInput {
    #[serde(default = "cols")]
    cols: u16,
    #[serde(default = "rows")]
    rows: u16,
    /// Open a TCP tunnel to this port on the endpoint instead of a shell.
    #[serde(default)]
    tunnel_port: Option<u16>,
}
fn cols() -> u16 {
    120
}
fn rows() -> u16 {
    32
}

#[derive(Serialize)]
pub struct Opened {
    session_id: Uuid,
    /// One-time ticket for the admin WebSocket (browsers can't send headers).
    ticket: String,
}

pub async fn open(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>, Json(inp): Json<OpenInput>) -> ApiResult<Json<Opened>> {
    let e = s.db.get_endpoint(id)?.ok_or_else(|| ApiError::not_found("Endpoint"))?;
    let ticket = random_token(24);
    let session_id = Uuid::new_v4();
    s.hub.sessions.lock().unwrap_or_else(|e| e.into_inner()).insert(
        session_id,
        Session {
            endpoint: id,
            host: e.hostname.clone(),
            admin: admin.name().to_string(),
            ticket_hash: Some(sha256_hex(ticket.as_bytes())),
            created: Instant::now(),
            cols: inp.cols.clamp(20, 400),
            rows: inp.rows.clamp(5, 200),
            tunnel_port: inp.tunnel_port.filter(|p| *p > 0),
            delivered: false,
            to_admin: None,
            to_agent: None,
            admin_backlog: vec![],
            agent_backlog: vec![],
        },
    );
    s.hub.wake(id);
    match inp.tunnel_port {
        Some(p) => s.db.audit(admin.name(), "tunnel.open", &e.hostname, &format!("TCP tunnel to port {p} ({})", port_name(p))),
        None => s.db.audit(admin.name(), "shell.open", &e.hostname, "Interactive remote shell"),
    }
    Ok(Json(Opened { session_id, ticket }))
}

fn port_name(p: u16) -> &'static str {
    match p {
        3389 => "Remote Desktop",
        5900 => "VNC / Screen Sharing",
        22 => "SSH",
        5985 | 5986 => "WinRM",
        _ => "custom",
    }
}

#[derive(Deserialize)]
pub struct TicketQuery {
    ticket: String,
}

/// The admin side. Authenticated with the one-time ticket from `open`.
pub async fn admin_ws(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<TicketQuery>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    {
        let mut m = s.hub.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let sess = m.get_mut(&id).ok_or_else(|| ApiError::not_found("Shell session"))?;
        let ok = sess.ticket_hash.as_deref().map(|h| ct_eq(h, &sha256_hex(q.ticket.as_bytes()))).unwrap_or(false);
        if !ok {
            return Err(ApiError::unauthorized());
        }
        sess.ticket_hash = None; // single use
    }
    Ok(ws.on_upgrade(move |socket| relay(s, id, socket, true)))
}

/// The agent side.
pub async fn agent_ws(
    State(s): State<AppState>,
    EndpointCaller(auth): EndpointCaller,
    Path(id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    let owner = s.hub.sessions.lock().unwrap_or_else(|e| e.into_inner()).get(&id).map(|x| x.endpoint);
    if owner != Some(auth.id) {
        return Err(ApiError::not_found("Shell session"));
    }
    Ok(ws.on_upgrade(move |socket| relay(s, id, socket, false)))
}

async fn relay(s: AppState, id: Uuid, socket: WebSocket, is_admin: bool) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    let backlog = {
        let mut m = s.hub.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let Some(sess) = m.get_mut(&id) else { return };
        if is_admin {
            sess.to_admin = Some(tx);
            std::mem::take(&mut sess.admin_backlog)
        } else {
            sess.to_agent = Some(tx);
            std::mem::take(&mut sess.agent_backlog)
        }
    };
    for m in backlog {
        if sink.send(m).await.is_err() {
            return;
        }
    }
    let started = Instant::now();
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            let close = matches!(m, Message::Close(_));
            if sink.send(m).await.is_err() || close {
                break;
            }
        }
    });
    while let Some(Ok(msg)) = stream.next().await {
        let close = matches!(msg, Message::Close(_));
        if matches!(msg, Message::Ping(_) | Message::Pong(_)) {
            continue;
        }
        let mut m = s.hub.sessions.lock().unwrap_or_else(|e| e.into_inner());
        let Some(sess) = m.get_mut(&id) else { break };
        let other = if is_admin { &sess.to_agent } else { &sess.to_admin };
        match other {
            Some(t) => {
                let _ = t.send(msg);
            }
            None if !close => {
                let buf = if is_admin { &mut sess.agent_backlog } else { &mut sess.admin_backlog };
                if buf.len() < 2000 {
                    buf.push(msg);
                }
            }
            None => {}
        }
        if close {
            break;
        }
    }
    // One side left: end the session for both.
    let ended = s.hub.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    if let Some(sess) = ended {
        for t in [sess.to_admin, sess.to_agent].into_iter().flatten() {
            let _ = t.send(Message::Close(None));
        }
        if is_admin {
            s.db.audit(&sess.admin, "shell.close", &sess.host, &format!("{} min", started.elapsed().as_secs().div_ceil(60)));
        }
    }
    writer.abort();
}

#[derive(Serialize)]
pub struct Wake {
    wake: bool,
}

/// Long poll: returns as soon as there's work for this endpoint, or after 25 s.
pub async fn wait(State(s): State<AppState>, EndpointCaller(auth): EndpointCaller) -> Json<Wake> {
    let n = s.hub.waker(auth.id);
    let woke = tokio::time::timeout(Duration::from_secs(25), n.notified()).await.is_ok();
    Json(Wake { wake: woke })
}
