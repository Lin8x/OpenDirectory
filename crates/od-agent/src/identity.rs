//! The agent's device certificate for the Internet Gateway (mutual TLS).
//!
//! The agent makes its own key, asks the Management Server to sign it, and
//! presents the certificate on every connection. The private key never
//! leaves the device. It's renewed 30 days before it expires.

use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use od_core::protocol::{CertEnrollRequest, CertEnrollResponse};

use crate::state::data_dir;

const RENEW_BEFORE_DAYS: i64 = 30;

fn paths() -> (std::path::PathBuf, std::path::PathBuf) {
    let d = data_dir().join("certs");
    (d.join("device.crt"), d.join("device.key"))
}

/// The saved identity and when its certificate expires.
pub fn load() -> Option<(od_client::Identity, Option<DateTime<Utc>>)> {
    let (c, k) = paths();
    let cert_pem = std::fs::read_to_string(c).ok()?;
    let key_pem = std::fs::read_to_string(k).ok()?;
    let not_after = std::fs::read_to_string(data_dir().join("certs").join("device.expires"))
        .ok()
        .and_then(|s| DateTime::parse_from_rfc3339(s.trim()).ok())
        .map(|d| d.with_timezone(&Utc));
    Some((od_client::Identity { cert_pem, key_pem }, not_after))
}

#[derive(Default)]
pub struct DeviceIdentity {
    not_after: Option<DateTime<Utc>>,
    loaded: bool,
    next_try: Option<Instant>,
}

impl DeviceIdentity {
    /// Use the saved certificate (call once at startup).
    pub fn init(&mut self, client: &od_client::Client) {
        self.loaded = true;
        if let Some((id, na)) = load() {
            self.not_after = na;
            if let Err(e) = client.set_identity(Some(id)) {
                tracing::warn!("device certificate: {e:#}");
            }
        }
    }

    /// Request or renew the certificate when needed.
    pub async fn maintain(&mut self, client: &od_client::Client) {
        if !self.loaded {
            self.init(client);
        }
        let due = self.not_after.map(|na| na - Utc::now() < chrono::Duration::days(RENEW_BEFORE_DAYS)).unwrap_or(true);
        if !due || self.next_try.is_some_and(|t| Instant::now() < t) {
            return;
        }
        match request(client).await {
            Ok(na) => {
                tracing::info!("Device certificate for the Internet Gateway valid until {}", na.format("%Y-%m-%d"));
                self.not_after = Some(na);
                self.next_try = None;
            }
            Err(e) => {
                let msg = format!("{e:#}");
                // An older Management Server doesn't have the endpoint; don't keep asking.
                let old_server = msg.contains("404");
                tracing::debug!("device certificate request failed: {msg}");
                self.next_try = Some(Instant::now() + Duration::from_secs(if old_server { 6 * 3600 } else { 300 }));
            }
        }
    }
}

async fn request(client: &od_client::Client) -> anyhow::Result<DateTime<Utc>> {
    let key = rcgen::KeyPair::generate()?;
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new())?;
    params.distinguished_name.push(rcgen::DnType::CommonName, "OpenDirectory Endpoint Agent");
    let csr = params.serialize_request(&key)?;
    let resp: CertEnrollResponse = client.post("/agent/device-certificate", &CertEnrollRequest { csr_pem: csr.pem()? }).await?;
    let (c, k) = paths();
    std::fs::create_dir_all(c.parent().unwrap())?;
    crate::certs::write_private(&k, key.serialize_pem().as_bytes())?;
    std::fs::write(&c, &resp.cert_pem)?;
    std::fs::write(c.with_file_name("device.expires"), resp.not_after.to_rfc3339())?;
    client.set_identity(Some(od_client::Identity { cert_pem: resp.cert_pem, key_pem: key.serialize_pem() }))?;
    Ok(resp.not_after)
}
