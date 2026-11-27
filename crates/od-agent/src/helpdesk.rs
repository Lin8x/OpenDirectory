//! Help desk chat on the device, without an app running in the background.
//!
//! When the Help desk GPO setting is on, the agent serves a small chat page
//! on 127.0.0.1 only and adds a "Contact IT" shortcut that opens it in the
//! browser. Messages go to the Management Server right away; replies arrive
//! with the next check-in (a reply wakes the agent) and pop up as a desktop
//! notification. When the setting is off, nothing listens and the shortcut
//! is removed.

use std::sync::{Arc, Mutex};

use od_core::policy::Setting;
use od_core::protocol::{ChatMessage, ChatPost};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::state::data_dir;

pub const PORT: u16 = 47800;
const KEEP: usize = 200;

#[derive(Default)]
struct Shared {
    messages: Vec<ChatMessage>,
    contact: String,
}

#[derive(Default)]
pub struct HelpDesk {
    shared: Arc<Mutex<Shared>>,
    server: Option<tokio::task::JoinHandle<()>>,
    loaded: bool,
    cleaned: bool,
}

fn history_path() -> std::path::PathBuf {
    data_dir().join("helpdesk.json")
}

fn lock(s: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

impl HelpDesk {
    /// The newest message id we have, for `CheckinRequest::chat_since`.
    pub fn since(&mut self) -> i64 {
        self.load();
        lock(&self.shared).messages.last().map(|m| m.id).unwrap_or(0)
    }

    fn load(&mut self) {
        if !self.loaded {
            self.loaded = true;
            if let Some(v) = std::fs::read(history_path()).ok().and_then(|b| serde_json::from_slice::<Vec<ChatMessage>>(&b).ok()) {
                lock(&self.shared).messages = v;
            }
        }
    }

    /// Apply policy and merge new messages from a check-in.
    pub async fn sync(&mut self, settings: &[Setting], incoming: Vec<ChatMessage>, client: &od_client::Client) {
        self.load();
        let contact = settings.iter().find_map(|s| match s {
            Setting::HelpDesk { enabled: true, contact } => Some(contact.clone()),
            _ => None,
        });
        if !incoming.is_empty() {
            let first_sync = self.since() == 0;
            let replies: Vec<ChatMessage> = {
                let mut sh = lock(&self.shared);
                let last = sh.messages.last().map(|m| m.id).unwrap_or(0);
                let fresh: Vec<ChatMessage> = incoming.into_iter().filter(|m| m.id > last).collect();
                sh.messages.extend(fresh.iter().cloned());
                let n = sh.messages.len();
                if n > KEEP {
                    sh.messages.drain(..n - KEEP);
                }
                let _ = std::fs::write(history_path(), serde_json::to_vec(&sh.messages).unwrap_or_default());
                fresh.into_iter().filter(|m| m.from_it).collect()
            };
            if contact.is_some() && !first_sync {
                for m in replies.iter().rev().take(1) {
                    let more = if replies.len() > 1 { format!(" (+{} more)", replies.len() - 1) } else { String::new() };
                    crate::desktop::notify(&format!("Message from IT: {}{more}", m.author), &m.text).await;
                }
            }
        }
        match contact {
            Some(c) => {
                lock(&self.shared).contact = c;
                if self.server.as_ref().map(|h| h.is_finished()).unwrap_or(true) {
                    match TcpListener::bind(("127.0.0.1", PORT)).await {
                        Ok(l) => {
                            tracing::info!("Help desk chat page on http://127.0.0.1:{PORT}/");
                            self.server = Some(tokio::spawn(serve(l, self.shared.clone(), client.clone())));
                            shortcut(true);
                        }
                        Err(e) => tracing::warn!("help desk: can't listen on 127.0.0.1:{PORT}: {e}"),
                    }
                }
            }
            None => {
                if let Some(h) = self.server.take() {
                    h.abort();
                    shortcut(false);
                    tracing::info!("Help desk chat turned off");
                } else if !self.cleaned {
                    // Turned off while the agent wasn't running.
                    shortcut(false);
                }
                self.cleaned = true;
            }
        }
    }
}

async fn serve(l: TcpListener, shared: Arc<Mutex<Shared>>, client: od_client::Client) {
    loop {
        let Ok((mut sock, _)) = l.accept().await else { continue };
        let (shared, client) = (shared.clone(), client.clone());
        tokio::spawn(async move {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(20), handle(&mut sock, &shared, &client)).await;
        });
    }
}

struct Req {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

async fn read_req(sock: &mut tokio::net::TcpStream) -> Option<Req> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    let head_end = loop {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 16 * 1024 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, path) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: Vec<(String, String)> =
        lines.filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))).collect();
    let len: usize = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    if len > 32 * 1024 {
        return None;
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = sock.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    Some(Req { method, path, headers, body })
}

async fn respond(sock: &mut tokio::net::TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = sock.write_all(head.as_bytes()).await;
    let _ = sock.write_all(body).await;
}

async fn handle(sock: &mut tokio::net::TcpStream, shared: &Mutex<Shared>, client: &od_client::Client) {
    let Some(req) = read_req(sock).await else { return };
    // Only this machine's browser, addressed by loopback name: blocks DNS
    // rebinding and other websites posting here.
    let host_ok = req.header("host").is_some_and(|h| h == format!("127.0.0.1:{PORT}") || h == format!("localhost:{PORT}"));
    let origin_ok =
        req.header("origin").map(|o| o == format!("http://127.0.0.1:{PORT}") || o == format!("http://localhost:{PORT}")).unwrap_or(true);
    if !host_ok || !origin_ok {
        return respond(sock, "403 Forbidden", "text/plain", b"Forbidden").await;
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => respond(sock, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()).await,
        ("GET", "/api/messages") => {
            let body = {
                let sh = lock(shared);
                serde_json::json!({ "messages": sh.messages, "contact": sh.contact })
            };
            respond(sock, "200 OK", "application/json", body.to_string().as_bytes()).await
        }
        ("POST", "/api/messages") => {
            if req.header("x-od-helpdesk") != Some("1") {
                return respond(sock, "403 Forbidden", "text/plain", b"Forbidden").await;
            }
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(&req.body) else {
                return respond(sock, "400 Bad Request", "text/plain", b"Bad request").await;
            };
            let text = v["text"].as_str().unwrap_or("").trim().to_string();
            if text.is_empty() {
                return respond(sock, "400 Bad Request", "text/plain", b"Type a message first").await;
            }
            let author = crate::desktop::console_user().await.map(|u| u.name).unwrap_or_default();
            match client.post::<ChatMessage, _>("/agent/chat", &ChatPost { text, author }).await {
                Ok(m) => {
                    {
                        let mut sh = lock(shared);
                        if sh.messages.last().map(|l| l.id < m.id).unwrap_or(true) {
                            sh.messages.push(m.clone());
                            let _ = std::fs::write(history_path(), serde_json::to_vec(&sh.messages).unwrap_or_default());
                        }
                    }
                    respond(sock, "200 OK", "application/json", serde_json::to_string(&m).unwrap_or_default().as_bytes()).await
                }
                Err(e) => {
                    let msg = format!("Couldn't send your message right now: {e}. Try again in a minute.");
                    respond(sock, "502 Bad Gateway", "text/plain; charset=utf-8", msg.as_bytes()).await
                }
            }
        }
        _ => respond(sock, "404 Not Found", "text/plain", b"Not found").await,
    }
}

/// Add or remove the "Contact IT" shortcut for every user.
fn shortcut(on: bool) {
    let url = format!("http://127.0.0.1:{PORT}/");
    #[cfg(target_os = "linux")]
    let (path, body) = (
        std::path::PathBuf::from("/usr/share/applications/opendirectory-contact-it.desktop"),
        format!("[Desktop Entry]\nType=Application\nName=Contact IT\nComment=Chat with your IT help desk\nExec=xdg-open {url}\nIcon=help-browser\nCategories=Utility;\nTerminal=false\n"),
    );
    #[cfg(target_os = "macos")]
    let (path, body) = (
        std::path::PathBuf::from("/Applications/Contact IT.webloc"),
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>URL</key><string>{url}</string></dict></plist>\n"),
    );
    #[cfg(windows)]
    let (path, body) = (
        std::path::PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| "C:\\ProgramData".into()))
            .join("Microsoft\\Windows\\Start Menu\\Programs\\Contact IT.url"),
        format!("[InternetShortcut]\r\nURL={url}\r\n"),
    );
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let (path, body) = (std::path::PathBuf::new(), url);
    if path.as_os_str().is_empty() {
        return;
    }
    let res = if on { std::fs::write(&path, body) } else { std::fs::remove_file(&path).or(Ok(())) };
    if let Err(e) = res {
        tracing::debug!("help desk shortcut {}: {e}", path.display());
    }
}

const PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Contact IT</title>
<style>
:root{color-scheme:light dark;--bg:#f6f7f9;--card:#fff;--ink:#1d2330;--mute:#677085;--me:#2f6fed;--line:#e2e5ec}
@media(prefers-color-scheme:dark){:root{--bg:#14171d;--card:#1c2029;--ink:#e7eaf0;--mute:#9aa3b5;--me:#3d7bf5;--line:#2b303b}}
*{box-sizing:border-box}body{margin:0;font:15px/1.45 system-ui,-apple-system,Segoe UI,sans-serif;background:var(--bg);color:var(--ink);display:flex;flex-direction:column;height:100vh}
header{padding:16px 20px;border-bottom:1px solid var(--line);background:var(--card)}h1{margin:0;font-size:18px}header p{margin:4px 0 0;color:var(--mute);font-size:13px}
main{flex:1;overflow:auto;padding:20px;display:flex;flex-direction:column;gap:10px}
.m{max-width:75%;padding:9px 13px;border-radius:14px;background:var(--card);border:1px solid var(--line);white-space:pre-wrap;word-wrap:break-word}
.me{align-self:flex-end;background:var(--me);color:#fff;border-color:transparent}.meta{font-size:11px;opacity:.7;margin-top:3px}
.empty{color:var(--mute);text-align:center;margin-top:40px}
form{display:flex;gap:8px;padding:12px 16px;border-top:1px solid var(--line);background:var(--card)}
textarea{flex:1;resize:none;height:44px;padding:10px;border-radius:10px;border:1px solid var(--line);background:var(--bg);color:var(--ink);font:inherit}
button{padding:0 18px;border:0;border-radius:10px;background:var(--me);color:#fff;font:inherit;font-weight:600;cursor:pointer}button:disabled{opacity:.5}
#err{color:#d14343;font-size:13px;padding:0 20px 8px;background:var(--card)}
</style></head><body>
<header><h1>Contact IT</h1><p>Messages go to your organization's IT team. They can see this device's name, not your files.<span id="contact"></span></p></header>
<main id="list"><div class="empty">No messages yet. Describe your problem below and IT will reply here.</div></main>
<div id="err"></div>
<form id="f"><textarea id="t" placeholder="Describe the problem…" maxlength="4000"></textarea><button id="b">Send</button></form>
<script>
const list=document.getElementById('list'),t=document.getElementById('t'),b=document.getElementById('b'),err=document.getElementById('err');let last=-1;
function render(d){const c=document.getElementById('contact');c.textContent=d.contact?' Other ways to reach IT: '+d.contact:'';
 const ms=d.messages||[];const top=ms.length?ms[ms.length-1].id:0;if(top===last)return;last=top;if(!ms.length)return;list.textContent='';
 for(const m of ms){const e=document.createElement('div');e.className='m'+(m.from_it?'':' me');e.textContent=m.text;const meta=document.createElement('div');meta.className='meta';
  meta.textContent=(m.from_it?m.author+' (IT) · ':'You · ')+new Date(m.at).toLocaleString();e.appendChild(meta);list.appendChild(e)}list.scrollTop=list.scrollHeight}
async function load(){try{const r=await fetch('/api/messages');render(await r.json())}catch(e){}}
document.getElementById('f').onsubmit=async e=>{e.preventDefault();const text=t.value.trim();if(!text)return;b.disabled=true;err.textContent='';
 try{const r=await fetch('/api/messages',{method:'POST',headers:{'Content-Type':'application/json','X-OD-Helpdesk':'1'},body:JSON.stringify({text})});
  if(!r.ok)throw new Error(await r.text());t.value='';await load()}catch(x){err.textContent=x.message}b.disabled=false};
t.addEventListener('keydown',e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();document.getElementById('f').requestSubmit()}});
load();setInterval(load,5000);
</script></body></html>
"#;
