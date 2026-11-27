//! Remote desktop in the browser.
//!
//! Three ways to see and control an endpoint's screen, all carried through
//! the agent's outbound tunnel, so the endpoint never opens an inbound port:
//!
//! * **VNC**: the browser runs noVNC and speaks RFB over the session
//!   WebSocket straight to the endpoint's VNC / Screen Sharing service.
//! * **RDP**: the browser runs the Guacamole client; this server relays it to
//!   `guacd` (Apache Guacamole's proxy daemon, run as a sidecar container),
//!   and guacd's RDP connection is carried to the endpoint's port 3389.
//! * **Stream**: the agent captures the screen and encodes it with the GPU
//!   (NVENC, VAAPI, Quick Sync, AMF or VideoToolbox, with a software
//!   fallback); the browser decodes it with WebCodecs. Lowest latency, for
//!   games and creative apps.

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

use super::shell::{start_session, take_ticket, Opened};
use crate::auth::Operator;
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use od_core::protocol::{StreamOptions, STREAM_PORT};

/// Where guacd listens (`OD_GUACD`, e.g. `guacd:4822`). Unset turns browser RDP off.
fn guacd_addr() -> Option<String> {
    std::env::var("OD_GUACD").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The address guacd uses to reach this server for the RDP leg
/// (`OD_GUACD_CALLBACK`, default: this host's name in Compose).
fn callback_host() -> String {
    std::env::var("OD_GUACD_CALLBACK").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "127.0.0.1".into())
}

#[derive(Serialize)]
pub struct Capabilities {
    vnc: bool,
    rdp: bool,
    stream: bool,
    /// Why RDP isn't available, for the UI.
    rdp_note: Option<String>,
}

pub async fn capabilities(_op: Operator) -> Json<Capabilities> {
    let rdp = guacd_addr().is_some();
    Json(Capabilities {
        vnc: true,
        rdp,
        stream: true,
        rdp_note: (!rdp).then(|| {
            "Browser RDP needs the guacd service. It's included in the Docker Compose file; for other installs set OD_GUACD.".into()
        }),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Vnc,
    Rdp,
    Stream,
}

#[derive(Deserialize)]
pub struct StartInput {
    protocol: Protocol,
    #[serde(default)]
    stream: Option<StreamOptions>,
    /// RDP sign-in for the endpoint. Held in memory until guacd uses it,
    /// never stored or logged.
    #[serde(default)]
    rdp: Option<RdpLogin>,
}

#[derive(Deserialize, Default, Clone)]
pub struct RdpLogin {
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    domain: String,
}

/// RDP sign-ins waiting for their session's WebSocket (dropped after 2 min).
static RDP_LOGINS: std::sync::Mutex<Option<std::collections::HashMap<Uuid, (RdpLogin, std::time::Instant)>>> = std::sync::Mutex::new(None);

fn take_login(id: Uuid) -> RdpLogin {
    let mut g = RDP_LOGINS.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(Default::default);
    m.retain(|_, (_, at)| at.elapsed() < Duration::from_secs(120));
    m.remove(&id).map(|(l, _)| l).unwrap_or_default()
}

#[derive(Serialize)]
pub struct Started {
    #[serde(flatten)]
    opened: Opened,
    /// WebSocket path to connect to (relative to the API base).
    ws_path: String,
}

pub async fn start(State(s): State<AppState>, op: Operator, Path(id): Path<Uuid>, Json(inp): Json<StartInput>) -> ApiResult<Json<Started>> {
    let e = super::endpoints::visible(&s, &op, id)?;
    let (port, stream) = match inp.protocol {
        Protocol::Vnc => (5900, None),
        Protocol::Rdp => {
            if guacd_addr().is_none() {
                return Err(ApiError::bad_request(
                    "Browser RDP isn't set up on this server. Run it with the guacd service (see the Docker guide) or use VNC or Stream.",
                ));
            }
            (3389, None)
        }
        Protocol::Stream => (STREAM_PORT, Some(inp.stream.unwrap_or_default())),
    };
    let opened = start_session(&s, &op, &e, 80, 24, Some(port), stream)?;
    if matches!(inp.protocol, Protocol::Rdp) {
        RDP_LOGINS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert_with(Default::default)
            .insert(opened.session_id, (inp.rdp.unwrap_or_default(), std::time::Instant::now()));
    }
    let ws_path = match inp.protocol {
        Protocol::Rdp => format!("/api/v1/remote/{}/guac", opened.session_id),
        _ => format!("/api/v1/shell/{}/ws?ticket={}", opened.session_id, opened.ticket),
    };
    Ok(Json(Started { opened, ws_path }))
}

#[derive(Deserialize)]
pub struct GuacQuery {
    ticket: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    dpi: Option<u32>,
}

/// The browser side of browser RDP, speaking the Guacamole protocol.
/// Authenticated with the session's one-time ticket.
pub async fn guac_ws(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<GuacQuery>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    let guacd = guacd_addr().ok_or_else(|| ApiError::bad_request("Browser RDP isn't set up on this server"))?;
    take_ticket(&s, id, &q.ticket)?;
    let login = take_login(id);
    Ok(ws.protocols(["guacamole"]).on_upgrade(move |socket| async move {
        if let Err(e) = guac_session(s.clone(), id, socket, &guacd, q, login).await {
            tracing::warn!("browser RDP session {id}: {e:#}");
            s.hub.end(id);
        }
    }))
}

async fn guac_session(s: AppState, id: Uuid, mut browser: WebSocket, guacd: &str, q: GuacQuery, login: RdpLogin) -> anyhow::Result<()> {
    // 1. A one-shot listener that guacd's RDP connection lands on; it's joined
    //    to the agent's tunnel to port 3389.
    let guacd_ips: Vec<std::net::IpAddr> = tokio::net::lookup_host(guacd).await?.map(|a| a.ip()).collect();
    let listener = TcpListener::bind(("0.0.0.0", 0)).await?;
    let rdp_port = listener.local_addr()?.port();

    // 2. Handshake with guacd.
    let mut g = TcpStream::connect(guacd).await.map_err(|e| anyhow::anyhow!("can't reach guacd at {guacd}: {e}"))?;
    g.write_all(encode(&["select", "rdp"]).as_bytes()).await?;
    let mut reader = InstructionReader::default();
    let args = loop {
        let ins = reader.next(&mut g).await?;
        if ins.first().map(String::as_str) == Some("args") {
            break ins;
        }
    };
    let (w, h, dpi) =
        (q.width.unwrap_or(1280).clamp(320, 7680), q.height.unwrap_or(720).clamp(240, 4320), q.dpi.unwrap_or(96).clamp(48, 384));
    let (w, h, dpi) = (w.to_string(), h.to_string(), dpi.to_string());
    g.write_all(encode(&["size", &w, &h, &dpi]).as_bytes()).await?;
    g.write_all(encode(&["audio", "audio/L16"]).as_bytes()).await?;
    g.write_all(encode(&["video"]).as_bytes()).await?;
    g.write_all(encode(&["image", "image/png", "image/jpeg", "image/webp"]).as_bytes()).await?;
    let host = callback_host();
    let port = rdp_port.to_string();
    let values: Vec<String> = args[1..]
        .iter()
        .map(|name| {
            let v = match name.as_str() {
                n if n.starts_with("VERSION_") => n,
                "hostname" => &host,
                "port" => &port,
                "username" => &login.username,
                "password" => &login.password,
                "domain" => &login.domain,
                "security" => "any",
                "ignore-cert" => "true",
                "resize-method" => "display-update",
                "enable-font-smoothing" => "true",
                "enable-desktop-composition" => "true",
                "enable-full-window-drag" => "true",
                "enable-menu-animations" => "false",
                _ => "",
            };
            v.to_string()
        })
        .collect();
    let mut connect = vec!["connect"];
    connect.extend(values.iter().map(String::as_str));
    g.write_all(encode(&connect).as_bytes()).await?;

    // 3. guacd now dials our listener; only accept it from guacd itself.
    let accept = async {
        loop {
            let (tcp, peer) = listener.accept().await?;
            if guacd_ips.contains(&peer.ip()) || peer.ip().is_loopback() && guacd_ips.iter().any(|i| i.is_loopback()) {
                return anyhow::Ok(tcp);
            }
            tracing::warn!("browser RDP: refused a connection from {peer}, which isn't guacd");
        }
    };
    let ready = async {
        loop {
            let ins = reader.next(&mut g).await?;
            match ins.first().map(String::as_str) {
                Some("ready") => return anyhow::Ok(()),
                Some("error") => anyhow::bail!("guacd: {}", ins.get(1).cloned().unwrap_or_default()),
                _ => {}
            }
        }
    };
    let (rdp, ()) = tokio::time::timeout(Duration::from_secs(60), async { tokio::try_join!(accept, ready) })
        .await
        .map_err(|_| anyhow::anyhow!("timed out waiting for the endpoint's Remote Desktop connection"))??;
    drop(listener);
    let (rd, wr) = rdp.into_split();
    let to_tcp = futures_util::sink::unfold(wr, |mut wr, m: Message| async move {
        if let Message::Binary(b) = m {
            wr.write_all(&b).await?;
        }
        Ok::<_, std::io::Error>(wr)
    });
    let from_tcp = tokio_util::io::ReaderStream::new(rd).map(|r| r.map(Message::Binary));
    tokio::spawn(super::shell::relay_io(s.clone(), id, Box::pin(to_tcp), from_tcp, true));

    // 4. Relay browser <-> guacd. The JavaScript tunnel wants whole
    //    instructions per WebSocket message, and an initial tunnel id.
    browser.send(Message::Text(encode(&["", &id.to_string()]).into())).await?;
    let (mut bsink, mut bstream) = browser.split();
    let (mut grd, mut gwr) = g.into_split();
    let leftover = reader.take_buffer();
    let down = async move {
        let mut r = InstructionReader::with(leftover);
        loop {
            let batch = r.next_batch(&mut grd).await?;
            if bsink.send(Message::Text(batch.into())).await.is_err() {
                break;
            }
        }
        anyhow::Ok(())
    };
    let up = async move {
        while let Some(Ok(m)) = bstream.next().await {
            match m {
                Message::Text(t) => {
                    // Internal tunnel instructions (pings) stay here.
                    if t.starts_with("0.,") {
                        continue;
                    }
                    gwr.write_all(t.as_bytes()).await?;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
        anyhow::Ok(())
    };
    tokio::select! { r = down => r?, r = up => r? }
    s.hub.end(id);
    Ok(())
}

/// Encode one Guacamole instruction (lengths count Unicode characters).
fn encode(parts: &[&str]) -> String {
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&p.chars().count().to_string());
        out.push('.');
        out.push_str(p);
    }
    out.push(';');
    out
}

/// Reads complete Guacamole instructions from guacd.
#[derive(Default)]
struct InstructionReader {
    buf: String,
    pending: Vec<u8>,
}

impl InstructionReader {
    fn with(buf: String) -> Self {
        InstructionReader { buf, pending: vec![] }
    }
    fn take_buffer(&mut self) -> String {
        std::mem::take(&mut self.buf)
    }

    /// Length in bytes of the first complete instruction in `buf`, if any.
    fn complete(buf: &str) -> anyhow::Result<Option<usize>> {
        let mut chars = buf.char_indices().peekable();
        loop {
            let mut len = 0usize;
            let mut digits = 0;
            loop {
                match chars.next() {
                    Some((_, c)) if c.is_ascii_digit() => {
                        len = len * 10 + (c as usize - '0' as usize);
                        digits += 1;
                        if digits > 9 {
                            anyhow::bail!("malformed Guacamole instruction");
                        }
                    }
                    Some((_, '.')) if digits > 0 => break,
                    Some(_) => anyhow::bail!("malformed Guacamole instruction"),
                    None => return Ok(None),
                }
            }
            for _ in 0..len {
                if chars.next().is_none() {
                    return Ok(None);
                }
            }
            match chars.next() {
                Some((i, ';')) => return Ok(Some(i + 1)),
                Some((_, ',')) => continue,
                Some(_) => anyhow::bail!("malformed Guacamole instruction"),
                None => return Ok(None),
            }
        }
    }

    async fn fill(&mut self, src: &mut (impl AsyncReadExt + Unpin)) -> anyhow::Result<()> {
        let mut tmp = [0u8; 16384];
        let n = src.read(&mut tmp).await?;
        if n == 0 {
            anyhow::bail!("guacd closed the connection");
        }
        self.pending.extend_from_slice(&tmp[..n]);
        // Keep any incomplete UTF-8 sequence for the next read.
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(s) => s.len(),
            Err(e) => e.valid_up_to(),
        };
        self.buf.push_str(std::str::from_utf8(&self.pending[..valid]).expect("checked"));
        self.pending.drain(..valid);
        if self.buf.len() > 32 * 1024 * 1024 {
            anyhow::bail!("Guacamole instruction too large");
        }
        Ok(())
    }

    async fn next(&mut self, src: &mut (impl AsyncReadExt + Unpin)) -> anyhow::Result<Vec<String>> {
        loop {
            if let Some(end) = Self::complete(&self.buf)? {
                let ins: String = self.buf.drain(..end).collect();
                return Ok(decode(&ins));
            }
            self.fill(src).await?;
        }
    }

    /// Every complete instruction available, as one string.
    async fn next_batch(&mut self, src: &mut (impl AsyncReadExt + Unpin)) -> anyhow::Result<String> {
        loop {
            let mut total = 0;
            while let Some(end) = Self::complete(&self.buf[total..])? {
                total += end;
            }
            if total > 0 {
                return Ok(self.buf.drain(..total).collect());
            }
            self.fill(src).await?;
        }
    }
}

fn decode(ins: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = ins;
    while let Some((len, after)) = rest.split_once('.') {
        let Ok(n) = len.parse::<usize>() else { break };
        let value: String = after.chars().take(n).collect();
        let used = value.len();
        out.push(value);
        rest = &after[used..];
        rest = rest.strip_prefix(',').unwrap_or(rest);
        if rest.starts_with(';') || rest.is_empty() {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guacamole_instructions_roundtrip() {
        let e = encode(&["size", "1280", "720", "96"]);
        assert_eq!(e, "4.size,4.1280,3.720,2.96;");
        assert_eq!(decode(&e), ["size", "1280", "720", "96"]);
        // Lengths count characters, not bytes.
        let u = encode(&["name", "Café"]);
        assert_eq!(u, "4.name,4.Café;");
        assert_eq!(InstructionReader::complete(&u).unwrap(), Some(u.len()));
        assert_eq!(InstructionReader::complete("4.name,4.Caf").unwrap(), None);
        assert_eq!(InstructionReader::complete("4.sync,3.123;5.mouse").unwrap(), Some(13));
        assert!(InstructionReader::complete("x.bad;").is_err());
        assert_eq!(encode(&["", "abc"]), "0.,3.abc;");
    }
}
