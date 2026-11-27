//! Internet Gateway: lets Endpoint Agents reach the Management Server from
//! anywhere, not only the office network.
//!
//! It's a second TLS listener (port 8444 by default) that you forward from
//! your router or firewall. It is locked down three ways:
//!
//! 1. **Agent routes only.** The Admin Center API, sign-in and web UI aren't
//!    served here, so the admin surface never faces the internet.
//! 2. **Mutual TLS.** Every enrolled agent gets a device certificate from a
//!    built-in Device Identity CA. Apart from enrollment itself, requests
//!    without a valid, registered device certificate are refused. The
//!    certificate must also belong to the endpoint named in the request's
//!    credentials, so a stolen endpoint secret is useless without the key
//!    that never leaves the device (and the other way around).
//! 3. **Pinned server certificate.** Agents pin the server's fingerprint, as
//!    on the LAN, so no public CA or DNS name is needed.
//!
//! Failed enrollments are rate limited per IP address. Agents keep the LAN
//! address first and fall back to the gateway when it can't be reached.

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{AddExtension, Next};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json, Router};
use axum_server::accept::Accept;
use axum_server::tls_rustls::{RustlsAcceptor, RustlsConfig};
use chrono::Utc;
use rcgen::{
    BasicConstraints, CertificateParams, CertificateSigningRequestParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose, SerialNumber,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::server::TlsStream;
use tower::Layer;
use uuid::Uuid;

use crate::crypto::sha256_hex;
use crate::AppState;

pub const DEFAULT_PORT: u16 = 8444;
/// Device certificates are valid this long; agents renew 30 days before.
const CERT_DAYS: i64 = 365;

/// Saved in the database, so replica servers can run a gateway too.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GatewayConfig {
    pub enabled: bool,
    /// Port to listen on (all interfaces).
    #[serde(default)]
    pub port: u16,
    /// How agents reach it from outside, e.g. `https://od.example.com:8444`.
    #[serde(default)]
    pub public_url: String,
}

impl GatewayConfig {
    pub fn load(s: &AppState) -> GatewayConfig {
        let mut c: GatewayConfig =
            s.db.meta_get("internet_gateway").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
        if c.port == 0 {
            c.port = DEFAULT_PORT;
        }
        c
    }
}

/// The running listener, if any, and why it isn't running otherwise.
#[derive(Default)]
pub struct Runtime {
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub status: Mutex<String>,
    /// The server's TLS certificate and key (PEM), shared with the main listener.
    pub tls: Mutex<Option<(String, String)>>,
    failures: Mutex<HashMap<std::net::IpAddr, (u32, Instant)>>,
}

impl Runtime {
    fn set_status(&self, s: impl Into<String>) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = s.into();
    }
    pub fn status(&self) -> String {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

// ------------------------------------------------------------ Device Identity CA

fn to_time(t: chrono::DateTime<Utc>) -> time::OffsetDateTime {
    time::OffsetDateTime::from_unix_timestamp(t.timestamp()).unwrap_or(time::OffsetDateTime::now_utc())
}

fn ca_params() -> CertificateParams {
    let mut p = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "OpenDirectory Device Identity CA");
    dn.push(DnType::OrganizationName, "OpenDirectory");
    p.distinguished_name = dn;
    p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    p
}

/// The Device Identity CA, created on first use. Its key is sealed with the
/// server's master key and stored in the database.
pub fn device_ca(s: &AppState) -> Result<(rcgen::Certificate, KeyPair, String)> {
    if let (Some(cert_pem), Some(sealed)) = (s.db.meta_get("device_ca_cert")?, s.db.meta_get("device_ca_key")?) {
        let key_pem = String::from_utf8(s.sealer.open(&hex::decode(sealed)?)?)?;
        let kp = KeyPair::from_pem(&key_pem).map_err(|e| anyhow::anyhow!("device CA key: {e}"))?;
        // Re-create the issuer from the stored parameters; the stored PEM is what clients trust.
        let params = CertificateParams::from_ca_cert_pem(&cert_pem).map_err(|e| anyhow::anyhow!("device CA: {e}"))?;
        let cert = params.self_signed(&kp).map_err(|e| anyhow::anyhow!("{e}"))?;
        return Ok((cert, kp, cert_pem));
    }
    let kp = KeyPair::generate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut p = ca_params();
    let now = Utc::now();
    p.not_before = to_time(now - chrono::Duration::hours(1));
    p.not_after = to_time(now + chrono::Duration::days(20 * 365));
    let cert = p.self_signed(&kp).map_err(|e| anyhow::anyhow!("{e}"))?;
    let pem = cert.pem();
    s.db.meta_set("device_ca_key", &hex::encode(s.sealer.seal(kp.serialize_pem().as_bytes())?))?;
    s.db.meta_set("device_ca_cert", &pem)?;
    s.db.audit("System", "gateway.device_ca", "Device Identity CA", "Created the CA that issues Endpoint Agent device certificates");
    Ok((cert, kp, pem))
}

/// Sign an agent's CSR. The subject is the endpoint ID, whatever the CSR says.
pub fn issue_device_cert(s: &AppState, endpoint: Uuid, csr_pem: &str) -> Result<(String, String, chrono::DateTime<Utc>)> {
    let (ca, ca_key, ca_pem) = device_ca(s)?;
    let mut csr = CertificateSigningRequestParams::from_pem(csr_pem.trim()).map_err(|e| anyhow::anyhow!("Invalid CSR: {e}"))?;
    let now = Utc::now();
    let not_after = now + chrono::Duration::days(CERT_DAYS);
    let mut serial = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut serial);
    serial[0] &= 0x7f;
    let p = &mut csr.params;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, endpoint.to_string());
    dn.push(DnType::OrganizationName, "OpenDirectory Endpoint");
    p.distinguished_name = dn;
    p.subject_alt_names = vec![];
    p.not_before = to_time(now - chrono::Duration::minutes(5));
    p.not_after = to_time(not_after);
    p.serial_number = Some(SerialNumber::from_slice(&serial));
    p.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    p.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    p.is_ca = IsCa::ExplicitNoCa;
    let c = csr.signed_by(&ca, &ca_key).map_err(|e| anyhow::anyhow!("{e}"))?;
    s.db.add_device_cert(endpoint, &sha256_hex(c.der()), not_after)?;
    Ok((c.pem(), ca_pem, not_after))
}

// ------------------------------------------------------------ listener

/// The device certificate a connection presented (SHA-256 of its DER).
#[derive(Debug, Clone)]
pub struct PeerCert(pub Option<String>);

#[derive(Clone)]
struct MtlsAcceptor {
    inner: RustlsAcceptor,
}

impl<I, S> Accept<I, S> for MtlsAcceptor
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    S: Send + 'static,
{
    type Stream = TlsStream<I>;
    type Service = AddExtension<S, PeerCert>;
    type Future = Pin<Box<dyn Future<Output = std::io::Result<(Self::Stream, Self::Service)>> + Send>>;

    fn accept(&self, stream: I, service: S) -> Self::Future {
        let acceptor = self.inner.clone();
        Box::pin(async move {
            let (stream, service) = acceptor.accept(stream, service).await?;
            let fp = stream.get_ref().1.peer_certificates().and_then(|c| c.first()).map(|c| sha256_hex(c.as_ref()));
            Ok((stream, Extension(PeerCert(fp)).layer(service)))
        })
    }
}

fn tls_config(s: &AppState, cert_pem: &str, key_pem: &str) -> Result<Arc<rustls::ServerConfig>> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let (_, _, ca_pem) = device_ca(s)?;
    let mut roots = rustls::RootCertStore::empty();
    for c in CertificateDer::pem_slice_iter(ca_pem.as_bytes()) {
        roots.add(c?)?;
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    // Unauthenticated clients may connect, so a new device can enroll; the
    // middleware below refuses everything else without a device certificate.
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .allow_unauthenticated()
        .build()
        .context("device certificate verifier")?;
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(cert_pem.as_bytes()).collect::<Result<_, _>>()?;
    let key = PrivateKeyDer::from_pem_slice(key_pem.as_bytes()).context("server TLS key")?;
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_client_cert_verifier(verifier)
        .with_single_cert(chain, key)?;
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(cfg))
}

/// Start, restart or stop the gateway to match the saved configuration.
pub async fn apply(s: &AppState) {
    let cfg = GatewayConfig::load(s);
    let rt = s.gateway.clone();
    let old = rt.task.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(t) = old {
        t.abort();
        // Give the old listener a moment to release the port.
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if !cfg.enabled {
        rt.set_status("Off");
        return;
    }
    let tls_pem = rt.tls.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let Some((cert, key)) = tls_pem else {
        rt.set_status("Needs TLS: the Management Server is running with --insecure-http");
        return;
    };
    let tls = match tls_config(s, &cert, &key) {
        Ok(t) => t,
        Err(e) => {
            rt.set_status(format!("Error: {e:#}"));
            return;
        }
    };
    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = match std::net::TcpListener::bind(addr).and_then(|l| l.set_nonblocking(true).map(|_| l)) {
        Ok(l) => l,
        Err(e) => {
            rt.set_status(format!("Can't listen on port {}: {e}", cfg.port));
            return;
        }
    };
    let app = router(s.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let server = axum_server::from_tcp(listener).acceptor(MtlsAcceptor { inner: RustlsAcceptor::new(RustlsConfig::from_config(tls)) });
    rt.set_status(format!("Listening on port {}", cfg.port));
    tracing::info!("Internet Gateway listening on {addr} (agents only, mutual TLS)");
    let rt2 = rt.clone();
    let port = cfg.port;
    *rt.task.lock().unwrap_or_else(|e| e.into_inner()) = Some(tokio::spawn(async move {
        if let Err(e) = server.serve(app).await {
            tracing::error!("Internet Gateway stopped: {e}");
            rt2.set_status(format!("Stopped on port {port}: {e}"));
        }
    }));
}

/// Only the agent API, behind the device certificate check.
fn router(state: AppState) -> Router {
    crate::api::router(state.clone()).layer(axum::middleware::from_fn_with_state(state, guard))
}

fn deny(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}

async fn guard(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let Some(agent_path) = path.strip_prefix("/api/v1/agent/") else {
        return deny(StatusCode::NOT_FOUND, "The Internet Gateway only serves Endpoint Agents");
    };
    let peer = req.extensions().get::<axum::extract::ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    let cert = req.extensions().get::<PeerCert>().and_then(|p| p.0.clone());
    if agent_path == "enroll" {
        if let Some(ip) = peer {
            let mut f = s.gateway.failures.lock().unwrap_or_else(|e| e.into_inner());
            f.retain(|_, (_, t)| t.elapsed() < Duration::from_secs(600));
            if f.get(&ip).is_some_and(|(n, _)| *n >= 10) {
                return deny(StatusCode::TOO_MANY_REQUESTS, "Too many failed enrollments from this address; try again in 10 minutes");
            }
        }
        let resp = next.run(req).await;
        if !resp.status().is_success() {
            if let Some(ip) = peer {
                let mut f = s.gateway.failures.lock().unwrap_or_else(|e| e.into_inner());
                let e = f.entry(ip).or_insert((0, Instant::now()));
                e.0 += 1;
                e.1 = Instant::now();
            }
        }
        return resp;
    }
    // Everything else names an endpoint in its credentials.
    let endpoint = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Endpoint "))
        .and_then(|c| c.split_once('.'))
        .and_then(|(id, _)| Uuid::parse_str(id).ok());
    let Some(endpoint) = endpoint else { return deny(StatusCode::UNAUTHORIZED, "Sign in required") };
    let owner = match &cert {
        Some(fp) => s.db.device_cert_owner(fp).ok().flatten(),
        None => None,
    };
    match (cert.is_some(), owner) {
        (true, Some(o)) if o == endpoint => next.run(req).await,
        (true, _) => deny(StatusCode::FORBIDDEN, "This device certificate doesn't belong to this endpoint"),
        // A device enrolled over the internet asks for its first certificate.
        (false, _) if agent_path == "device-certificate" && s.db.device_cert_count(endpoint).unwrap_or(1) == 0 => next.run(req).await,
        (false, _) => deny(StatusCode::FORBIDDEN, "A device certificate is required on the Internet Gateway"),
    }
}

// ------------------------------------------------------------ API

#[derive(Serialize)]
pub struct GatewayView {
    #[serde(flatten)]
    config: GatewayConfig,
    status: String,
    tls_fingerprint: Option<String>,
    /// Endpoints that already have a device certificate.
    devices_with_certificates: i64,
    endpoints: i64,
}

pub async fn get(State(s): State<AppState>, _a: crate::auth::Admin) -> crate::error::ApiResult<Json<GatewayView>> {
    Ok(Json(GatewayView {
        config: GatewayConfig::load(&s),
        status: s.gateway.status(),
        tls_fingerprint: s.info.tls_fingerprint.clone(),
        devices_with_certificates: s.db.device_cert_endpoints()?,
        endpoints: s.db.list_endpoints()?.len() as i64,
    }))
}

pub async fn set(
    State(s): State<AppState>,
    admin: crate::auth::Admin,
    Json(mut c): Json<GatewayConfig>,
) -> crate::error::ApiResult<Json<GatewayView>> {
    use crate::error::ApiError;
    if c.port == 0 {
        c.port = DEFAULT_PORT;
    }
    c.public_url = c.public_url.trim().trim_end_matches('/').to_string();
    if c.enabled {
        if !c.public_url.starts_with("https://") {
            return Err(ApiError::bad_request("The public address must start with https://, e.g. https://od.example.com:8444"));
        }
        let rest = &c.public_url["https://".len()..];
        if rest.is_empty() || rest.contains(['/', ' ', '@', '?', '#']) {
            return Err(ApiError::bad_request("The public address is just https://host:port, with no path"));
        }
    }
    s.db.meta_set("internet_gateway", &serde_json::to_string(&c).map_err(anyhow::Error::from)?)?;
    s.db.audit(
        admin.name(),
        if c.enabled { "gateway.enable" } else { "gateway.disable" },
        &c.public_url,
        &format!("Internet Gateway on port {}", c.port),
    );
    apply(&s).await;
    get(State(s), admin).await
}

/// The agent asks for (or renews) its device certificate.
pub async fn agent_certificate(
    State(s): State<AppState>,
    crate::auth::EndpointCaller(auth): crate::auth::EndpointCaller,
    Json(req): Json<od_core::protocol::CertEnrollRequest>,
) -> crate::error::ApiResult<Json<od_core::protocol::CertEnrollResponse>> {
    let (cert_pem, ca_pem, not_after) =
        issue_device_cert(&s, auth.id, &req.csr_pem).map_err(|e| crate::error::ApiError::bad_request(format!("{e:#}")))?;
    Ok(Json(od_core::protocol::CertEnrollResponse { cert_pem, ca_pem, not_after }))
}

/// Where agents can reach this server from the internet, if anywhere.
pub fn public_endpoint(s: &AppState) -> Option<od_core::protocol::ServerEndpoint> {
    let c = GatewayConfig::load(s);
    (c.enabled && !c.public_url.is_empty() && s.info.tls_fingerprint.is_some()).then(|| od_core::protocol::ServerEndpoint {
        url: c.public_url,
        fingerprint: s.info.tls_fingerprint.clone(),
        primary: false,
    })
}
