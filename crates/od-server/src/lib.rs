//! OpenDirectory Management Server.
//!
//! Holds the directory (users, security groups, OUs), endpoints, Group
//! Policy and remote scripts, and serves the management API used by the
//! Admin Center, the `od` CLI and Endpoint Agents. It runs standalone
//! (`opendirectory-server`) or embedded inside the Admin Center app.

pub mod api;
pub mod auth;
pub mod backup;
pub mod crypto;
pub mod db;
pub mod destinations;
pub mod error;
pub mod gateway;
pub mod netsvc;
pub mod service;
pub mod tls;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use tokio::net::TcpListener;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// How often agents check in, in seconds.
pub const CHECKIN_INTERVAL_SECS: u64 = 15;

#[derive(Clone)]
pub struct AppState {
    pub db: db::Db,
    pub sealer: crypto::Sealer,
    pub info: Arc<ServerInfo>,
    /// Failed Admin Center sign-ins per username, for lockout.
    pub login_failures: Arc<std::sync::Mutex<std::collections::HashMap<String, (u32, std::time::Instant)>>>,
    /// Remote shell sessions and agent wake-ups.
    pub hub: Arc<api::shell::Hub>,
    /// DNS and other hosted network services.
    pub services: Arc<netsvc::Services>,
    pub replication: Arc<api::replication::Runtime>,
    /// The Internet Gateway listener for agents outside the LAN.
    pub gateway: Arc<gateway::Runtime>,
}

/// Facts about this server shown in the Admin Center and enrollment commands.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServerInfo {
    pub version: &'static str,
    /// URL Endpoint Agents and remote admins use to reach this server.
    pub endpoint_url: String,
    /// SHA-256 fingerprint of the TLS certificate, for pinning.
    pub tls_fingerprint: Option<String>,
    pub data_dir: String,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub data_dir: PathBuf,
    /// Address for Endpoint Agents and remote admins (TLS when enabled).
    pub listen: SocketAddr,
    pub tls: bool,
    /// Extra plain-HTTP listener bound to loopback, used by an embedding
    /// Admin Center on the same machine. `None` disables it.
    pub local_listen: Option<SocketAddr>,
    /// Hostname or IP put in enrollment commands. Detected when empty.
    pub public_host: Option<String>,
    /// Folder with the built Admin Center web UI. When set, the server also
    /// serves the Admin Center to browsers (web mode).
    pub web_ui: Option<PathBuf>,
    /// Use this certificate and key (PEM) instead of the self-signed one.
    pub tls_cert: Option<(PathBuf, PathBuf)>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            data_dir: PathBuf::from("data"),
            listen: "0.0.0.0:8443".parse().unwrap(),
            tls: true,
            local_listen: None,
            public_host: None,
            web_ui: None,
            tls_cert: None,
        }
    }
}

pub struct RunningServer {
    pub info: Arc<ServerInfo>,
    /// Address of the loopback listener, if one was requested.
    pub local_addr: Option<SocketAddr>,
    pub listen_addr: SocketAddr,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl RunningServer {
    /// Wait until the server stops (normally never).
    pub async fn wait(self) {
        for t in self.tasks {
            let _ = t.await;
        }
    }
    /// Stop all listeners and release their ports.
    pub fn shutdown(self) {
        for t in self.tasks {
            t.abort();
        }
    }
    pub fn local_url(&self) -> Option<String> {
        self.local_addr.map(|a| format!("http://{a}"))
    }
}

pub fn build_state(data_dir: &std::path::Path, info: ServerInfo) -> Result<AppState> {
    std::fs::create_dir_all(data_dir).with_context(|| format!("create data dir {}", data_dir.display()))?;
    if let Some(kept) = backup::apply_pending(data_dir)? {
        tracing::info!("Restored from backup; the previous data is in {}", kept.display());
    }
    let db = db::Db::open(&data_dir.join("directory.db"))?;
    let sealer = crypto::Sealer::load_or_create(&data_dir.join("master.key"))?;
    Ok(AppState {
        db,
        sealer,
        info: Arc::new(info),
        login_failures: Default::default(),
        hub: Default::default(),
        services: Default::default(),
        replication: Arc::new(api::replication::Runtime::load(data_dir)),
        gateway: Default::default(),
    })
}

/// Background work: scheduled backups (checked every minute) and the daily
/// audit log retention chosen at onboarding.
fn spawn_housekeeping(state: AppState) {
    let s2 = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            // Scheduled backups run on the primary only.
            if !s2.replication.is_replica() {
                api::backups::run_due(&s2).await;
            }
        }
    });
    tokio::spawn(async move {
        loop {
            if let Ok(Some(d)) = state.db.meta_get("audit_retention_days") {
                if let Ok(days) = d.parse::<u32>() {
                    match state.db.prune_audit(days.max(1)) {
                        Ok(n) if n > 0 => tracing::info!("audit retention: removed {n} entries older than {days} days"),
                        Err(e) => tracing::warn!("audit retention: {e:#}"),
                        _ => {}
                    }
                }
            }
            match api::recycle::expire(&state) {
                Ok(n) if n > 0 => tracing::info!("recycle bin: purged {n} expired object(s)"),
                Err(e) => tracing::warn!("recycle bin: {e:#}"),
                _ => {}
            }
            tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}

pub fn router(state: AppState) -> Router {
    api::router(state)
}

/// Serve the built Admin Center for browsers, with strict security headers.
/// Unknown paths fall back to index.html (the UI routes on the URL hash).
pub fn with_web_ui(app: Router, dir: &std::path::Path) -> Router {
    use axum::http::{header, HeaderValue};
    use tower_http::services::{ServeDir, ServeFile};
    use tower_http::set_header::SetResponseHeaderLayer;
    let ui = Router::new()
        .fallback_service(ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html"))))
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'"),
        ))
        .layer(SetResponseHeaderLayer::overriding(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")))
        .layer(SetResponseHeaderLayer::overriding(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")));
    app.merge(ui)
}

fn detect_host() -> String {
    // Find the primary outbound interface address without sending anything.
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.0.2.1:9")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "localhost".into())
}

/// Start the Management Server and return once it is listening.
pub async fn start(config: ServerConfig) -> Result<RunningServer> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    std::fs::create_dir_all(&config.data_dir)?;
    let host = config.public_host.clone().unwrap_or_else(detect_host);

    let listener = std::net::TcpListener::bind(config.listen).with_context(|| format!("bind {}", config.listen))?;
    listener.set_nonblocking(true)?;
    let listen_addr = listener.local_addr()?;

    let mut tls_pem = None;
    let (scheme, tls_cfg, fingerprint) = if config.tls {
        let (cfg, fp) = match &config.tls_cert {
            Some((cert, key)) => tls::load_provided(cert, key).await?,
            None => tls::load_or_create(&config.data_dir, &host).await?,
        };
        let (cert, key) = match &config.tls_cert {
            Some((c, k)) => (c.clone(), k.clone()),
            None => (config.data_dir.join("tls-cert.pem"), config.data_dir.join("tls-key.pem")),
        };
        tls_pem = Some((std::fs::read_to_string(cert)?, std::fs::read_to_string(key)?));
        ("https", Some(cfg), Some(fp))
    } else {
        ("http", None, None)
    };

    let info = ServerInfo {
        version: VERSION,
        endpoint_url: format!("{scheme}://{host}:{}", listen_addr.port()),
        tls_fingerprint: fingerprint,
        data_dir: config.data_dir.display().to_string(),
    };
    let state = build_state(&config.data_dir, info)?;
    let info = state.info.clone();
    *state.gateway.tls.lock().unwrap_or_else(|e| e.into_inner()) = tls_pem;
    gateway::apply(&state).await;
    spawn_housekeeping(state.clone());
    api::integrations::spawn_dispatcher(state.clone());
    api::replication::init(&state);
    netsvc::start_all(&state);
    api::replication::spawn_sync(state.clone());
    api::samba::spawn_sync(state.clone());
    let mut app = router(state);
    if let Some(dir) = config.web_ui.as_ref().filter(|d| d.join("index.html").exists()) {
        tracing::info!("Serving the Admin Center web UI from {}", dir.display());
        app = with_web_ui(app, dir);
    }

    let mut tasks = Vec::new();
    let svc = app.clone().into_make_service_with_connect_info::<SocketAddr>();
    match tls_cfg {
        Some(cfg) => {
            let server = axum_server::from_tcp_rustls(listener, cfg);
            tasks.push(tokio::spawn(async move {
                if let Err(e) = server.serve(svc).await {
                    tracing::error!("server stopped: {e}");
                }
            }));
        }
        None => {
            let l = TcpListener::from_std(listener)?;
            tasks.push(tokio::spawn(async move {
                if let Err(e) = axum::serve(l, svc).await {
                    tracing::error!("server stopped: {e}");
                }
            }));
        }
    }

    let mut local_addr = None;
    if let Some(addr) = config.local_listen {
        let l = TcpListener::bind(addr).await.with_context(|| format!("bind {addr}"))?;
        local_addr = Some(l.local_addr()?);
        let svc = app.into_make_service_with_connect_info::<SocketAddr>();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = axum::serve(l, svc).await {
                tracing::error!("local listener stopped: {e}");
            }
        }));
    }

    tracing::info!("Management Server {} listening on {} ({})", VERSION, listen_addr, info.endpoint_url);
    Ok(RunningServer { info, local_addr, listen_addr, tasks })
}
