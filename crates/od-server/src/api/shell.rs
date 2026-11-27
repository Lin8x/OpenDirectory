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

use crate::auth::{EndpointCaller, Operator};
use crate::crypto::{ct_eq, random_token, sha256_hex};
use crate::error::{ApiError, ApiResult};
use crate::rbac::Permission;
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
    stream: Option<od_core::protocol::StreamOptions>,
    /// Exec into this container instead of a host shell (endpoint management).
    exec: Option<od_core::manage::ContainerExec>,
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
    /// End a session and close both sides.
    pub fn end(&self, id: Uuid) {
        if let Some(sess) = self.sessions.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
            for t in [sess.to_admin, sess.to_agent].into_iter().flatten() {
                let _ = t.send(Message::Close(None));
            }
        }
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
                od_core::protocol::ShellRequest {
                    session_id: *id,
                    cols: s.cols,
                    rows: s.rows,
                    tunnel_port: s.tunnel_port,
                    requested_by: s.admin.clone(),
                    stream: s.stream.clone(),
                    exec: s.exec.clone(),
                }
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
    /// Low-latency stream settings (with `tunnel_port` = the stream marker).
    #[serde(default)]
    stream: Option<od_core::protocol::StreamOptions>,
}
fn cols() -> u16 {
    120
}
fn rows() -> u16 {
    32
}

#[derive(Serialize)]
pub struct Opened {
    pub session_id: Uuid,
    /// One-time ticket for the admin WebSocket (browsers can't send headers).
    pub ticket: String,
}

pub async fn open(State(s): State<AppState>, admin: Operator, Path(id): Path<Uuid>, Json(inp): Json<OpenInput>) -> ApiResult<Json<Opened>> {
    let e = super::endpoints::visible(&s, &admin, id)?;
    Ok(Json(start_session(&s, &admin, &e, inp.cols, inp.rows, inp.tunnel_port, inp.stream)?))
}

/// Check permissions and policy, then queue a shell or tunnel session for
/// the endpoint's agent to pick up.
pub(crate) fn start_session(
    s: &AppState,
    admin: &Operator,
    e: &od_core::Endpoint,
    cols: u16,
    rows: u16,
    tunnel_port: Option<u16>,
    stream: Option<od_core::protocol::StreamOptions>,
) -> ApiResult<Opened> {
    let id = e.id;
    admin.need_endpoint(e, if tunnel_port.is_some() { Permission::RemoteDesktop } else { Permission::RemoteShell })?;
    // Delegated operators reach only the remote desktop services, not any
    // port on the endpoint's loopback.
    if !admin.access.domain_admin && tunnel_port.is_some_and(|p| !DESKTOP_PORTS.contains(&p)) {
        return Err(ApiError::forbidden("Delegated operators can only open remote desktop (RDP, VNC or stream) sessions"));
    }
    if e.os == od_core::OsFamily::Android {
        return Err(ApiError::bad_request("Android devices don't support remote shell or remote desktop sessions"));
    }
    // Remote tools can be turned off per endpoint or platform by GPO.
    for st in super::policies::endpoint_settings(s, e)? {
        if let od_core::policy::Setting::RemoteTools { remote_shell, remote_desktop } = st {
            if tunnel_port.is_some() && !remote_desktop {
                return Err(ApiError::forbidden("Remote desktop is turned off for this endpoint by the Remote tools GPO setting"));
            }
            if tunnel_port.is_none() && !remote_shell {
                return Err(ApiError::forbidden("Remote shell is turned off for this endpoint by the Remote tools GPO setting"));
            }
        }
    }
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
            cols: cols.clamp(20, 400),
            rows: rows.clamp(5, 200),
            tunnel_port: tunnel_port.filter(|p| *p > 0),
            stream: if tunnel_port == Some(od_core::protocol::STREAM_PORT) { Some(stream.unwrap_or_default()) } else { None },
            exec: None,
            delivered: false,
            to_admin: None,
            to_agent: None,
            admin_backlog: vec![],
            agent_backlog: vec![],
        },
    );
    s.hub.wake(id);
    match tunnel_port {
        Some(p) => s.db.audit(admin.name(), "tunnel.open", &e.hostname, &format!("TCP tunnel to port {p} ({})", port_name(p))),
        None => s.db.audit(admin.name(), "shell.open", &e.hostname, "Interactive remote shell"),
    }
    Ok(Opened { session_id, ticket })
}

/// A terminal inside a container (`docker exec -it`). The caller has checked
/// Manage containers; this checks Remote shell and the Remote tools setting.
pub(crate) fn start_exec(
    s: &AppState,
    admin: &Operator,
    e: &od_core::Endpoint,
    cols: u16,
    rows: u16,
    exec: od_core::manage::ContainerExec,
) -> ApiResult<Opened> {
    admin.need_endpoint(e, Permission::RemoteShell)?;
    for st in super::policies::endpoint_settings(s, e)? {
        if let od_core::policy::Setting::RemoteTools { remote_shell: false, .. } = st {
            return Err(ApiError::forbidden("Remote shell is turned off for this endpoint by the Remote tools GPO setting"));
        }
    }
    let ticket = random_token(24);
    let session_id = Uuid::new_v4();
    s.hub.sessions.lock().unwrap_or_else(|e| e.into_inner()).insert(
        session_id,
        Session {
            endpoint: e.id,
            host: e.hostname.clone(),
            admin: admin.name().to_string(),
            ticket_hash: Some(sha256_hex(ticket.as_bytes())),
            created: Instant::now(),
            cols: cols.clamp(20, 400),
            rows: rows.clamp(5, 200),
            tunnel_port: None,
            stream: None,
            exec: Some(exec),
            delivered: false,
            to_admin: None,
            to_agent: None,
            admin_backlog: vec![],
            agent_backlog: vec![],
        },
    );
    s.hub.wake(e.id);
    Ok(Opened { session_id, ticket })
}

/// Check and use up a session's one-time ticket.
pub(crate) fn take_ticket(s: &AppState, id: Uuid, ticket: &str) -> ApiResult<()> {
    let mut m = s.hub.sessions.lock().unwrap_or_else(|e| e.into_inner());
    let sess = m.get_mut(&id).ok_or_else(|| ApiError::not_found("Remote session"))?;
    let ok = sess.ticket_hash.as_deref().map(|h| ct_eq(h, &sha256_hex(ticket.as_bytes()))).unwrap_or(false);
    if !ok {
        return Err(ApiError::unauthorized());
    }
    sess.ticket_hash = None; // single use
    Ok(())
}

fn port_name(p: u16) -> &'static str {
    match p {
        3389 => "Remote Desktop",
        od_core::protocol::STREAM_PORT => "low-latency desktop stream",
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
    take_ticket(&s, id, &q.ticket)?;
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
    let (sink, stream) = socket.split();
    relay_io(s, id, sink, stream, is_admin).await
}

/// Join one side of a session to the hub. The admin side can be a browser
/// WebSocket, or a TCP connection from guacd for browser RDP.
pub(crate) async fn relay_io<Si, St, E>(s: AppState, id: Uuid, mut sink: Si, mut stream: St, is_admin: bool)
where
    Si: futures_util::Sink<Message> + Unpin + Send + 'static,
    St: futures_util::Stream<Item = Result<Message, E>> + Unpin,
{
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

/// Long poll: returns as soon as there's work for this endpoint, or after
/// 25 s. Management queries ride along so they're answered without a check-in.
pub async fn wait(State(s): State<AppState>, EndpointCaller(auth): EndpointCaller) -> Json<od_core::protocol::WaitResponse> {
    let n = s.hub.waker(auth.id);
    let q = super::manage::waker(auth.id);
    let mut queries = super::manage::take_queries(auth.id);
    let mut wake = false;
    if queries.is_empty() {
        wake = tokio::time::timeout(Duration::from_secs(25), async {
            tokio::select! {
                _ = n.notified() => true,
                _ = q.notified() => false,
            }
        })
        .await
        .unwrap_or(false);
        queries = super::manage::take_queries(auth.id);
    }
    Json(od_core::protocol::WaitResponse { wake, queries })
}

/// Ports a delegated operator may tunnel to: RDP, VNC and the agent's
/// low-latency stream host.
pub const DESKTOP_PORTS: [u16; 3] = [3389, 5900, od_core::protocol::STREAM_PORT];
