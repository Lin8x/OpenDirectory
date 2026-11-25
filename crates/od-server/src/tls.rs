//! Self-signed TLS for the Management Server. Clients pin the certificate's
//! SHA-256 fingerprint, so no public CA or internet access is needed.

use std::path::Path;

use anyhow::{Context, Result};
use axum_server::tls_rustls::RustlsConfig;

use crate::crypto::{sha256_hex, write_private};

pub fn fingerprint_of_pem(cert_pem: &str) -> Result<String> {
    use base64::Engine;
    let b64: String = cert_pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(b64).context("decode certificate")?;
    Ok(format!("sha256:{}", sha256_hex(&der)))
}

pub async fn load_or_create(dir: &Path, host: &str) -> Result<(RustlsConfig, String)> {
    let cert_path = dir.join("tls-cert.pem");
    let key_path = dir.join("tls-key.pem");
    if !cert_path.exists() || !key_path.exists() {
        let hostname = hostname_guess();
        let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string(), host.to_string()];
        if !hostname.is_empty() {
            names.push(hostname);
        }
        names.sort();
        names.dedup();
        let ck = rcgen::generate_simple_self_signed(names).context("generate certificate")?;
        std::fs::write(&cert_path, ck.cert.pem())?;
        write_private(&key_path, ck.key_pair.serialize_pem().as_bytes())?;
    }
    let pem = std::fs::read_to_string(&cert_path)?;
    let fp = fingerprint_of_pem(&pem)?;
    let cfg = RustlsConfig::from_pem_file(&cert_path, &key_path).await.context("load TLS certificate")?;
    Ok((cfg, fp))
}

fn hostname_guess() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Use a certificate and key you supply (e.g. from your own CA or Let's Encrypt).
pub async fn load_provided(cert: &Path, key: &Path) -> Result<(RustlsConfig, String)> {
    let pem = std::fs::read_to_string(cert).with_context(|| format!("read {}", cert.display()))?;
    let first = pem.split("-----END CERTIFICATE-----").next().unwrap_or("").to_string() + "-----END CERTIFICATE-----";
    let fp = fingerprint_of_pem(&first)?;
    let cfg = RustlsConfig::from_pem_file(cert, key).await.context("load the TLS certificate and key")?;
    Ok((cfg, fp))
}
