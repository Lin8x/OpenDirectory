//! OpenDirectory Management Server.
//!
//! Holds the directory (users, security groups, OUs), endpoints, Group
//! Policy and remote scripts, and serves the management API used by the
//! Admin Center, the `od` CLI and Endpoint Agents. It runs standalone
//! (`opendirectory-server`) or embedded inside the Admin Center app.

pub mod api;
pub mod auth;
pub mod crypto;
pub mod db;
pub mod error;
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
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            data_dir: PathBuf::from("data"),
            listen: "0.0.0.0:8443".parse().unwrap(),
            tls: true,
            local_listen: None,
            public_host: None,
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
    let db = db::Db::open(&data_dir.join("directory.db"))?;
    let sealer = crypto::Sealer::load_or_create(&data_dir.join("master.key"))?;
    Ok(AppState { db, sealer, info: Arc::new(info), login_failures: Default::default() })
}

/// Daily maintenance: apply the audit log retention chosen at onboarding.
fn spawn_housekeeping(state: AppState) {
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
            tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}

pub fn router(state: AppState) -> Router {
    api::router(state)
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

    let (scheme, tls_cfg, fingerprint) = if config.tls {
        let (cfg, fp) = tls::load_or_create(&config.data_dir, &host).await?;
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
    spawn_housekeeping(state.clone());
    let app = router(state);

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
