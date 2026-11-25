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
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
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
pub struct Client {
    http: reqwest::Client,
    base: String,
    auth: Option<String>,
}

impl Client {
    pub fn new(target: &ServerTarget) -> Result<Client> {
        let base = target.url.trim_end_matches('/').to_string();
        let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(60)).connect_timeout(Duration::from_secs(10)).user_agent(concat!("OpenDirectory/", env!("CARGO_PKG_VERSION")));
        if base.starts_with("https://") {
            let fp = target.fingerprint.as_deref().map(normalize_fingerprint).filter(|f| f.len() == 64).ok_or_else(|| {
                anyhow!("An https:// server needs its TLS fingerprint (shown in the Admin Center under Settings)")
            })?;
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let cfg = rustls::ClientConfig::builder_with_provider(provider.clone())
                .with_safe_default_protocol_versions()
                .context("TLS versions")?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(PinnedVerifier { fingerprint: fp, provider }))
                .with_no_client_auth();
            builder = builder.use_preconfigured_tls(cfg);
        } else if !base.starts_with("http://") {
            bail!("Server URL must start with https:// or http://");
        }
        Ok(Client { http: builder.build()?, base, auth: None })
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

    pub fn base_url(&self) -> &str {
        &self.base
    }

    /// Send a request to `/api/v1{path}` and return the JSON body (or `null`).
    pub async fn request(&self, method: Method, path: &str, body: Option<&serde_json::Value>) -> Result<serde_json::Value, ApiError> {
        let url = format!("{}/api/v1{}", self.base, path);
        let mut req = self.http.request(method, &url);
        if let Some(a) = &self.auth {
            req = req.header("authorization", a);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|e| ApiError { status: None, message: describe_reqwest(&e) })?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let json: serde_json::Value = if text.is_empty() { serde_json::Value::Null } else { serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)) };
        if status.is_success() {
            Ok(json)
        } else {
            let message = json.get("error").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| format!("Server returned {status}"));
            Err(ApiError { status: Some(status), message })
        }
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
