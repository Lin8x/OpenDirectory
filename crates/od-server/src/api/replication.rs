//! Replica servers and failover. One server is the primary and accepts
//! changes; replica servers copy its whole directory (database and secret
//! key) every minute and serve agents, sign-in, DNS, RADIUS and LDAP from
//! that copy, read-only. Agents learn every server and fail over on their
//! own. If the primary is lost, an admin promotes a replica; the epoch goes
//! up so agents and the old primary can tell which server is current.

use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

use axum::extract::{Path as UrlPath, Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::Admin;
use crate::crypto::{ct_eq, random_token, sha256_hex};
use crate::error::{ApiError, ApiResult};
use crate::AppState;

const STATE_FILE: &str = "replication.json";
/// How often a replica copies the primary.
pub const SYNC_EVERY: Duration = Duration::from_secs(60);
/// A replica counts as online if it synced within this window.
const ONLINE_SECS: i64 = 180;
/// Tables a replica keeps from its own copy (Admin Center sign-ins made on it).
const KEEP_TABLES: &[&str] = &["sessions"];

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Peer {
    pub url: String,
    pub fingerprint: Option<String>,
}

/// This server's own replication settings. Kept in a file, not the
/// database, because a replica's database is replaced on every sync.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct LocalState {
    #[serde(default)]
    pub replica: bool,
    pub primary: Option<Peer>,
    pub replica_id: Option<Uuid>,
    pub token: Option<String>,
    pub last_sync: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_error: String,
    /// Another server was promoted while this one was primary.
    #[serde(default)]
    pub superseded: bool,
}

#[derive(Default)]
pub struct Runtime {
    dir: PathBuf,
    state: RwLock<LocalState>,
    sync_lock: tokio::sync::Mutex<()>,
}

impl Runtime {
    pub fn load(dir: &Path) -> Runtime {
        let state = std::fs::read(dir.join(STATE_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Runtime { dir: dir.to_path_buf(), state: RwLock::new(state), sync_lock: Default::default() }
    }
    pub fn get(&self) -> LocalState {
        self.state.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn is_replica(&self) -> bool {
        self.get().replica
    }
    fn update(&self, f: impl FnOnce(&mut LocalState)) {
        let mut st = self.state.write().unwrap_or_else(|e| e.into_inner());
        f(&mut st);
        if !self.dir.as_os_str().is_empty() {
            if let Ok(bytes) = serde_json::to_vec_pretty(&*st) {
                let _ = crate::crypto::write_private(&self.dir.join(STATE_FILE), &bytes);
            }
        }
    }
}

/// The domain's servers. Stored in the database, so replicas have it too.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Topology {
    #[serde(default)]
    pub epoch: u64,
    pub primary: Option<Peer>,
    #[serde(default)]
    pub replicas: Vec<ReplicaRecord>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReplicaRecord {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    token_hash: String,
    pub url: Option<String>,
    pub fingerprint: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
    #[serde(default)]
    pub version: String,
}

pub fn topology(s: &AppState) -> Topology {
    s.db.meta_get("replication").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}
fn save_topology(s: &AppState, t: &Topology) -> anyhow::Result<()> {
    s.db.meta_set("replication", &serde_json::to_string(t)?)
}

fn me(s: &AppState) -> Peer {
    Peer { url: s.info.endpoint_url.clone(), fingerprint: s.info.tls_fingerprint.clone() }
}

/// For agents: every server, primary first, and the epoch. Empty when the
/// domain has a single server.
pub fn servers_for_agents(s: &AppState) -> (Vec<od_core::protocol::ServerEndpoint>, u64) {
    let t = topology(s);
    if t.replicas.is_empty() {
        return (vec![], t.epoch);
    }
    let mut v = vec![];
    if let Some(p) = &t.primary {
        v.push(od_core::protocol::ServerEndpoint { url: p.url.clone(), fingerprint: p.fingerprint.clone(), primary: true });
    }
    for r in &t.replicas {
        if let Some(url) = &r.url {
            v.push(od_core::protocol::ServerEndpoint { url: url.clone(), fingerprint: r.fingerprint.clone(), primary: false });
        }
    }
    (v, t.epoch)
}

/// An agent has seen a newer epoch than ours: another server was promoted.
pub fn note_agent_epoch(s: &AppState, known: u64) {
    if !s.replication.is_replica() && known > topology(s).epoch && !s.replication.get().superseded {
        s.replication.update(|st| st.superseded = true);
        s.db.audit(
            "System",
            "replication.superseded",
            &s.info.endpoint_url,
            "Agents report that another server was promoted to primary. Make this server a replica of it.",
        );
    }
}

/// At startup: a primary records its own address so agents can find it.
pub fn init(s: &AppState) {
    if s.replication.is_replica() {
        return;
    }
    let mut t = topology(s);
    if t.primary.as_ref() != Some(&me(s)) && s.db.meta_get("domain").ok().flatten().is_some() {
        t.primary = Some(me(s));
        let _ = save_topology(s, &t);
    }
}

/// Copy the primary every minute while this server is a replica.
pub fn spawn_sync(s: AppState) {
    tokio::spawn(async move {
        loop {
            if s.replication.is_replica() {
                if let Err(e) = sync_now(&s).await {
                    tracing::warn!("replication: {e:#}");
                }
            }
            tokio::time::sleep(SYNC_EVERY).await;
        }
    });
}

#[derive(Serialize, Deserialize)]
pub struct PullRequest {
    id: Uuid,
    token: String,
    url: String,
    fingerprint: Option<String>,
    version: String,
}

#[derive(Serialize, Deserialize)]
pub struct PullResponse {
    database: String,
    key: String,
    epoch: u64,
}

fn client_for(p: &Peer) -> anyhow::Result<od_client::Client> {
    od_client::Client::new(&od_client::ServerTarget { url: p.url.clone(), fingerprint: p.fingerprint.clone() })
}

/// Fetch and apply one copy of the primary.
pub async fn sync_now(s: &AppState) -> anyhow::Result<()> {
    let _guard = s.replication.sync_lock.lock().await;
    let st = s.replication.get();
    let (Some(primary), Some(id), Some(token)) = (st.primary.clone(), st.replica_id, st.token.clone()) else {
        anyhow::bail!("This replica has no primary configured");
    };
    let res = pull(s, &primary, id, &token).await;
    match &res {
        Ok(()) => s.replication.update(|st| {
            st.last_sync = Some(Utc::now());
            st.last_error.clear();
        }),
        Err(e) => {
            let msg = format!("{e:#}");
            s.replication.update(|st| st.last_error = msg);
        }
    }
    res
}

async fn pull(s: &AppState, primary: &Peer, id: Uuid, token: &str) -> anyhow::Result<()> {
    let req = PullRequest { id, token: token.into(), url: me(s).url, fingerprint: me(s).fingerprint, version: crate::VERSION.into() };
    let resp: PullResponse = client_for(primary)?.post("/replication/pull", &req).await?;
    let gz = b64().decode(resp.database)?;
    let mut db = vec![];
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&gz[..]), &mut db)?;
    let key = b64().decode(resp.key)?;
    anyhow::ensure!(key.len() == 32, "The primary sent an invalid key");
    s.db.restore_from(&db, KEEP_TABLES)?;
    if !s.replication.dir.as_os_str().is_empty() {
        crate::crypto::write_private(&s.replication.dir.join("master.key"), &key)?;
    }
    s.sealer.set_key(&key)?;
    // Serve DNS, RADIUS and LDAP from the new copy. DHCP stays off on replicas.
    crate::netsvc::start_all(s);
    Ok(())
}

// ------------------------------------------------------------------ API

#[derive(Serialize)]
pub struct StatusView {
    role: &'static str,
    this_server: Peer,
    epoch: u64,
    primary: Option<Peer>,
    replicas: Vec<ReplicaView>,
    last_sync: Option<DateTime<Utc>>,
    last_error: String,
    superseded: bool,
    sync_every_secs: u64,
}

#[derive(Serialize)]
pub struct ReplicaView {
    id: Uuid,
    name: String,
    url: Option<String>,
    created_at: DateTime<Utc>,
    last_seen: Option<DateTime<Utc>>,
    online: bool,
    version: String,
}

fn view(s: &AppState) -> StatusView {
    let st = s.replication.get();
    let t = topology(s);
    StatusView {
        role: if st.replica { "replica" } else { "primary" },
        this_server: me(s),
        epoch: t.epoch,
        primary: if st.replica { st.primary.clone() } else { t.primary.clone() },
        replicas: t
            .replicas
            .iter()
            .map(|r| ReplicaView {
                id: r.id,
                name: r.name.clone(),
                url: r.url.clone(),
                created_at: r.created_at,
                last_seen: r.last_seen,
                online: r.last_seen.map(|t| (Utc::now() - t).num_seconds() < ONLINE_SECS).unwrap_or(false),
                version: r.version.clone(),
            })
            .collect(),
        last_sync: st.last_sync,
        last_error: st.last_error,
        superseded: st.superseded,
        sync_every_secs: SYNC_EVERY.as_secs(),
    }
}

pub async fn status(State(s): State<AppState>, _a: Admin) -> Json<StatusView> {
    Json(view(&s))
}

/// Public: whether this is a replica, for the sign-in screen.
pub async fn role(State(s): State<AppState>) -> Json<serde_json::Value> {
    let st = s.replication.get();
    Json(serde_json::json!({ "replica": st.replica, "primary": st.primary.map(|p| p.url) }))
}

#[derive(Deserialize)]
pub struct AddInput {
    name: String,
}

#[derive(Serialize, Deserialize)]
struct JoinCode {
    u: String,
    f: Option<String>,
    i: Uuid,
    t: String,
}

pub async fn add(State(s): State<AppState>, admin: Admin, Json(inp): Json<AddInput>) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    primary_only(&s)?;
    let name = inp.name.trim();
    if name.is_empty() || name.len() > 64 {
        return Err(ApiError::bad_request("Name the replica server, for example DC02"));
    }
    let token = random_token(32);
    let id = Uuid::new_v4();
    let mut t = topology(&s);
    t.primary = Some(me(&s));
    t.replicas.push(ReplicaRecord {
        id,
        name: name.into(),
        token_hash: sha256_hex(token.as_bytes()),
        url: None,
        fingerprint: None,
        created_at: Utc::now(),
        last_seen: None,
        version: String::new(),
    });
    save_topology(&s, &t)?;
    let code = JoinCode { u: me(&s).url, f: me(&s).fingerprint, i: id, t: token };
    let code = format!(
        "ODREPL-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&code).map_err(anyhow::Error::from)?)
    );
    s.db.audit(admin.name(), "replication.add", name, "Replica server added");
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "id": id, "join_code": code }))))
}

pub async fn remove(State(s): State<AppState>, admin: Admin, UrlPath(id): UrlPath<Uuid>) -> ApiResult<StatusCode> {
    primary_only(&s)?;
    let mut t = topology(&s);
    let Some(pos) = t.replicas.iter().position(|r| r.id == id) else { return Err(ApiError::not_found("Replica server")) };
    let r = t.replicas.remove(pos);
    save_topology(&s, &t)?;
    s.db.audit(admin.name(), "replication.remove", &r.name, "Replica server removed; it can no longer copy the directory");
    Ok(StatusCode::NO_CONTENT)
}

/// Called by a replica to fetch a copy. Authenticated by the replica's token.
pub async fn serve_pull(State(s): State<AppState>, Json(req): Json<PullRequest>) -> ApiResult<Json<PullResponse>> {
    if s.replication.is_replica() {
        return Err(ApiError::conflict("This server is a replica; copy from the primary"));
    }
    let mut t = topology(&s);
    let Some(r) = t.replicas.iter_mut().find(|r| r.id == req.id) else { return Err(ApiError::unauthorized()) };
    if !ct_eq(&r.token_hash, &sha256_hex(req.token.as_bytes())) {
        return Err(ApiError::unauthorized());
    }
    let first = r.last_seen.is_none();
    r.last_seen = Some(Utc::now());
    r.url = Some(req.url.chars().take(200).collect());
    r.fingerprint = req.fingerprint.map(|f| f.chars().take(100).collect());
    r.version = req.version.chars().take(32).collect();
    let name = r.name.clone();
    t.primary = Some(me(&s));
    save_topology(&s, &t)?;
    if first {
        s.db.audit("System", "replication.joined", &name, "Replica server made its first copy");
    }
    let db = s.db.snapshot()?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut gz, &db).map_err(anyhow::Error::from)?;
    let key = std::fs::read(Path::new(&s.info.data_dir).join("master.key")).map_err(anyhow::Error::from)?;
    Ok(Json(PullResponse { database: b64().encode(gz.finish().map_err(anyhow::Error::from)?), key: b64().encode(key), epoch: t.epoch }))
}

#[derive(Deserialize)]
pub struct JoinInput {
    join_code: String,
    /// Required when this server already has a directory: it will be replaced.
    #[serde(default)]
    replace_existing: bool,
}

/// Make this server a replica. Allowed without signing in only on a server
/// that hasn't been set up yet.
pub async fn join(State(s): State<AppState>, admin: Result<Admin, ApiError>, Json(inp): Json<JoinInput>) -> ApiResult<Json<StatusView>> {
    let admin = admin.ok();
    let set_up = s.db.meta_get("domain")?.is_some();
    if set_up && admin.is_none() {
        return Err(ApiError::unauthorized());
    }
    if set_up && !inp.replace_existing {
        return Err(ApiError::conflict("This server already has a directory. Joining replaces it with the primary's copy."));
    }
    let raw = inp.join_code.trim().strip_prefix("ODREPL-").ok_or_else(|| ApiError::bad_request("That isn't a replica join code"))?;
    let code: JoinCode = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| ApiError::bad_request("That join code is damaged"))?;
    let primary = Peer { url: code.u, fingerprint: code.f };
    if primary.url.trim_end_matches('/') == me(&s).url.trim_end_matches('/') {
        return Err(ApiError::bad_request("That join code is for this server"));
    }
    let before = s.replication.get();
    s.replication.update(|st| {
        st.replica = true;
        st.primary = Some(primary.clone());
        st.replica_id = Some(code.i);
        st.token = Some(code.t.clone());
        st.superseded = false;
        st.last_error.clear();
    });
    if let Err(e) = sync_now(&s).await {
        s.replication.update(|st| *st = before);
        return Err(ApiError::bad_request(format!("Couldn't copy from the primary: {e:#}")));
    }
    let who = admin.as_ref().map(|a| a.name().to_string()).unwrap_or_else(|| "Setup".into());
    s.db.audit(&who, "replication.join", &primary.url, "This server is now a replica");
    Ok(Json(view(&s)))
}

pub async fn sync(State(s): State<AppState>, _a: Admin) -> ApiResult<Json<StatusView>> {
    if !s.replication.is_replica() {
        return Err(ApiError::conflict("This server is the primary"));
    }
    sync_now(&s).await.map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    Ok(Json(view(&s)))
}

/// Make this replica the primary (failover). Raise the epoch so agents and
/// the old primary know which server is current.
pub async fn promote(State(s): State<AppState>, admin: Admin) -> ApiResult<Json<StatusView>> {
    if !s.replication.is_replica() {
        return Err(ApiError::conflict("This server is already the primary"));
    }
    let _guard = s.replication.sync_lock.lock().await;
    let st = s.replication.get();
    let mut t = topology(&s);
    t.epoch += 1;
    t.primary = Some(me(&s));
    t.replicas.retain(|r| Some(r.id) != st.replica_id);
    save_topology(&s, &t)?;
    s.replication.update(|st| {
        st.replica = false;
        st.primary = None;
        st.token = None;
        st.replica_id = None;
        st.superseded = false;
    });
    crate::netsvc::start_all(&s);
    s.db.audit(
        admin.name(),
        "replication.promote",
        &s.info.endpoint_url,
        &format!("Promoted to primary (epoch {}). Other replicas need new join codes from this server.", t.epoch),
    );
    Ok(Json(view(&s)))
}

fn primary_only(s: &AppState) -> ApiResult<()> {
    if s.replication.is_replica() {
        return Err(ApiError::conflict("This is a replica server. Make this change on the primary."));
    }
    Ok(())
}

/// Replicas are read-only: refuse changes except sign-in, agent check-ins
/// and replication itself.
pub async fn read_only_guard(State(s): State<AppState>, req: Request, next: Next) -> Response {
    if s.replication.is_replica() && !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        // Inside the nested router the /api/v1 prefix may already be stripped.
        let p = req.uri().path().trim_start_matches("/api/v1");
        let allowed = ["/auth/login", "/auth/logout", "/replication/", "/agent/checkin"].iter().any(|a| p.starts_with(a));
        if !allowed {
            let primary = s.replication.get().primary.map(|p| p.url).unwrap_or_default();
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": format!("This is a read-only replica server. Make changes on the primary ({primary}), or promote this replica if the primary is gone.") })),
            )
                .into_response();
        }
    }
    next.run(req).await
}
