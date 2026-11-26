//! Client for the OpenDirectory management API.
//!
//! The Management Server uses a self-signed certificate, so clients pin its
//! SHA-256 fingerprint (shown in the Admin Center and in enrollment
//! commands) instead of trusting a public CA. This works fully offline.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
pub use reqwest::{Method, StatusCode};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Normalize `sha256:AB:CD…` / `abcd…` to lowercase hex.
pub fn normalize_fingerprint(fp: &str) -> String {
    fp.trim().trim_start_matches("sha256:").trim_start_matches("SHA256:").replace(':', "").to_lowercase()
}

#[derive(Debug)]
struct PinnedVerifier {
    fingerprint: String,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let got = hex::encode(Sha256::digest(end_entity.as_ref()));
        if got == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(format!(
                "server certificate fingerprint sha256:{got} does not match the pinned sha256:{}",
                self.fingerprint
            )))
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// Connection details for a Management Server.
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
pub struct ServerTarget {
    /// e.g. `https://10.0.0.4:8443` or `http://127.0.0.1:8080`.
    pub url: String,
    /// Required for `https://` URLs.
    pub fingerprint: Option<String>,
}

#[derive(Clone)]
struct Conn {
    http: reqwest::Client,
    base: String,
    tls: Option<Arc<rustls::ClientConfig>>,
}

/// How long to stay on a fallback server before trying the preferred one again.
const RETRY_PREFERRED: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct Client {
    /// The preferred server first, then fallbacks (replica servers).
    conns: Arc<std::sync::RwLock<Vec<Conn>>>,
    active: Arc<std::sync::atomic::AtomicUsize>,
    switched_at: Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    auth: Option<String>,
}

/// A WebSocket to the Management Server (remote shell and tunnels).
pub type WebSocket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
pub use tokio_tungstenite::tungstenite::Message as WsMessage;

impl Client {
    pub fn new(target: &ServerTarget) -> Result<Client> {
        let conn = Self::conn(target)?;
        Ok(Client {
            conns: Arc::new(std::sync::RwLock::new(vec![conn])),
            active: Default::default(),
            switched_at: Default::default(),
            auth: None,
        })
    }

    /// Use these servers, in order of preference, and fail over between them
    /// when one can't be reached. Targets that can't be used are skipped.
    pub fn set_servers(&self, targets: &[ServerTarget]) {
        let conns: Vec<Conn> = targets.iter().filter_map(|t| Self::conn(t).ok()).collect();
        if conns.is_empty() {
            return;
        }
        let cur = self.base_url();
        let idx = conns.iter().position(|c| c.base == cur).unwrap_or(0);
        *self.conns.write().unwrap_or_else(|e| e.into_inner()) = conns;
        self.active.store(idx, std::sync::atomic::Ordering::SeqCst);
    }

    /// Stop using the current server and move to the next one.
    pub fn fail_over(&self) {
        let n = self.conns.read().unwrap_or_else(|e| e.into_inner()).len();
        if n > 1 {
            let next = (self.active.load(std::sync::atomic::Ordering::SeqCst) + 1) % n;
            self.active.store(next, std::sync::atomic::Ordering::SeqCst);
            *self.switched_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
        }
    }

    fn conn(target: &ServerTarget) -> Result<Conn> {
        let base = target.url.trim_end_matches('/').to_string();
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("OpenDirectory/", env!("CARGO_PKG_VERSION")));
        if base.starts_with("https://") {
            let fp = target
                .fingerprint
                .as_deref()
                .map(normalize_fingerprint)
                .filter(|f| f.len() == 64)
                .ok_or_else(|| anyhow!("An https:// server needs its TLS fingerprint (shown in the Admin Center under Settings)"))?;
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let cfg = rustls::ClientConfig::builder_with_provider(provider.clone())
                .with_safe_default_protocol_versions()
                .context("TLS versions")?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(PinnedVerifier { fingerprint: fp, provider }))
                .with_no_client_auth();
            let cfg = Arc::new(cfg);
            builder = builder.use_preconfigured_tls((*cfg).clone());
            return Ok(Conn { http: builder.build()?, base, tls: Some(cfg) });
        } else if !base.starts_with("http://") {
            bail!("Server URL must start with https:// or http://");
        }
        Ok(Conn { http: builder.build()?, base, tls: None })
    }

    fn active_conn(&self) -> (usize, Conn) {
        let conns = self.conns.read().unwrap_or_else(|e| e.into_inner());
        let i = self.active.load(std::sync::atomic::Ordering::SeqCst).min(conns.len() - 1);
        (i, conns[i].clone())
    }

    /// Authenticate as an administrator with a session token.
    pub fn with_session(mut self, token: &str) -> Self {
        self.auth = Some(format!("Bearer {token}"));
        self
    }

    /// Authenticate as an enrolled endpoint.
    pub fn with_endpoint(mut self, id: &str, secret: &str) -> Self {
        self.auth = Some(format!("Endpoint {id}.{secret}"));
        self
    }

    /// The server currently in use.
    pub fn base_url(&self) -> String {
        self.active_conn().1.base
    }

    /// Open a WebSocket to `/api/v1{path}`, with the same certificate pinning
    /// and credentials as normal requests.
    pub async fn websocket(&self, path: &str) -> Result<WebSocket> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let (_, conn) = self.active_conn();
        let url = format!("{}/api/v1{}", conn.base.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1), path);
        let mut req = url.into_client_request()?;
        if let Some(a) = &self.auth {
            req.headers_mut().insert("authorization", a.parse()?);
        }
        let connector = conn.tls.clone().map(tokio_tungstenite::Connector::Rustls);
        let (ws, _) =
            tokio::time::timeout(Duration::from_secs(20), tokio_tungstenite::connect_async_tls_with_config(req, None, false, connector))
                .await
                .map_err(|_| anyhow!("Timed out connecting to the Management Server"))??;
        Ok(ws)
    }

    /// Send a request to `/api/v1{path}` and return the JSON body (or `null`).
    pub async fn request(&self, method: Method, path: &str, body: Option<&serde_json::Value>) -> Result<serde_json::Value, ApiError> {
        use std::sync::atomic::Ordering;
        let (active, _) = self.active_conn();
        let count = self.conns.read().unwrap_or_else(|e| e.into_inner()).len();
        // Order: the preferred server first if it's time to retry it, then
        // the active one, then the rest.
        let retry_preferred = active != 0
            && self.switched_at.lock().unwrap_or_else(|e| e.into_inner()).map(|t| t.elapsed() > RETRY_PREFERRED).unwrap_or(true);
        let mut order: Vec<usize> = if retry_preferred { vec![0, active] } else { vec![active] };
        let rest: Vec<usize> = (0..count).filter(|i| !order.contains(i)).collect();
        order.extend(rest);
        let mut last_err = None;
        for i in order {
            let conn = match self.conns.read().unwrap_or_else(|e| e.into_inner()).get(i) {
                Some(c) => c.clone(),
                None => continue,
            };
            match self.send(&conn, method.clone(), path, body).await {
                Err(e) if e.status.is_none() && count > 1 => last_err = Some(e),
                other => {
                    if i != active {
                        self.active.store(i, Ordering::SeqCst);
                        *self.switched_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
                    } else if retry_preferred {
                        *self.switched_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
                    }
                    return other;
                }
            }
        }
        Err(last_err.unwrap_or(ApiError { status: None, message: "No Management Server is reachable".into() }))
    }

    async fn send(&self, conn: &Conn, method: Method, path: &str, body: Option<&serde_json::Value>) -> Result<serde_json::Value, ApiError> {
        let url = format!("{}/api/v1{}", conn.base, path);
        let mut req = conn.http.request(method, &url);
        if let Some(a) = &self.auth {
            req = req.header("authorization", a);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|e| ApiError { status: None, message: describe_reqwest(&e) })?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let json: serde_json::Value =
            if text.is_empty() { serde_json::Value::Null } else { serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)) };
        if status.is_success() {
            Ok(json)
        } else {
            let message =
                json.get("error").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| format!("Server returned {status}"));
            Err(ApiError { status: Some(status), message })
        }
    }

    /// Download a file from `/api/v1{path}` from the server in use.
    pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let (_, conn) = self.active_conn();
        let mut req = conn.http.get(format!("{}/api/v1{}", conn.base, path)).timeout(Duration::from_secs(600));
        if let Some(a) = &self.auth {
            req = req.header("authorization", a);
        }
        let resp = req.send().await.map_err(|e| anyhow!(describe_reqwest(&e)))?;
        if !resp.status().is_success() {
            bail!("Server returned {}", resp.status());
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        Ok(serde_json::from_value(self.request(Method::GET, path, None).await?)?)
    }
    pub async fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        Ok(serde_json::from_value(self.request(Method::POST, path, Some(&serde_json::to_value(body)?)).await?)?)
    }
    pub async fn put<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        Ok(serde_json::from_value(self.request(Method::PUT, path, Some(&serde_json::to_value(body)?)).await?)?)
    }
    pub async fn patch<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        Ok(serde_json::from_value(self.request(Method::PATCH, path, Some(&serde_json::to_value(body)?)).await?)?)
    }
    pub async fn delete(&self, path: &str) -> Result<()> {
        self.request(Method::DELETE, path, None).await?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct ApiError {
    pub status: Option<StatusCode>,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ApiError {}

fn describe_reqwest(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        msg = format!("{msg}: {s}");
        src = s.source();
    }
    if e.is_connect() {
        format!("Can't reach the Management Server ({msg})")
    } else if e.is_timeout() {
        "The Management Server didn't answer in time".into()
    } else {
        msg
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fingerprint_forms() {
        assert_eq!(normalize_fingerprint("sha256:AB:cd"), "abcd");
        assert_eq!(normalize_fingerprint(" abcd "), "abcd");
    }
    #[test]
    fn https_requires_fingerprint() {
        assert!(Client::new(&ServerTarget { url: "https://x:8443".into(), fingerprint: None }).is_err());
        assert!(Client::new(&ServerTarget { url: "http://x:8080".into(), fingerprint: None }).is_ok());
    }
}
