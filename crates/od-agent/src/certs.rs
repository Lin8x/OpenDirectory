//! Machine certificate auto-enrollment. The agent makes its own key, sends a
//! CSR to the Management Server's certificate authority, and renews the
//! certificate before it expires. The private key never leaves the endpoint.

use chrono::Utc;
use od_core::policy::Setting;
use od_core::protocol::{CertEnrollRequest, CertEnrollResponse};
use od_core::{ComplianceItem, ComplianceState};

use crate::state::{data_dir, AgentState};

fn item(state: ComplianceState, message: impl Into<String>) -> ComplianceItem {
    ComplianceItem { setting: "certificate_auto_enrollment".into(), state, message: message.into() }
}

fn write_private(path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Returns a compliance item when the setting applies, and whether the agent
/// state changed.
pub async fn maintain(settings: &[Setting], state: &mut AgentState, client: &od_client::Client) -> (Option<ComplianceItem>, bool) {
    let Some(Setting::CertificateAutoEnrollment { renew_days }) =
        settings.iter().find(|s| matches!(s, Setting::CertificateAutoEnrollment { .. }))
    else {
        return (None, false);
    };
    let dir = data_dir().join("certs");
    let cert_path = dir.join("machine.crt");
    let due = match state.cert_not_after {
        Some(na) if cert_path.exists() => na - Utc::now() <= chrono::Duration::days((*renew_days).clamp(1, 365) as i64),
        _ => true,
    };
    if !due {
        let na = state.cert_not_after.unwrap_or_else(Utc::now);
        return (
            Some(item(
                ComplianceState::Compliant,
                format!("Machine certificate valid until {} ({})", na.format("%Y-%m-%d"), cert_path.display()),
            )),
            false,
        );
    }
    match enroll(&dir, client).await {
        Ok(na) => {
            state.cert_not_after = Some(na);
            (
                Some(item(
                    ComplianceState::Applied,
                    format!("Enrolled a machine certificate valid until {} ({})", na.format("%Y-%m-%d"), cert_path.display()),
                )),
                true,
            )
        }
        Err(e) => (Some(item(ComplianceState::Failed, format!("Certificate enrollment failed: {e:#}"))), false),
    }
}

async fn enroll(dir: &std::path::Path, client: &od_client::Client) -> anyhow::Result<chrono::DateTime<Utc>> {
    let host = sysinfo::System::host_name().unwrap_or_else(|| "endpoint".into());
    let key = rcgen::KeyPair::generate()?;
    let mut params = rcgen::CertificateParams::new(vec![host.clone()])?;
    params.distinguished_name.push(rcgen::DnType::CommonName, host);
    let csr = params.serialize_request(&key)?;
    let resp: CertEnrollResponse = client.post("/agent/certificate", &CertEnrollRequest { csr_pem: csr.pem()? }).await?;
    std::fs::create_dir_all(dir)?;
    write_private(&dir.join("machine.key"), key.serialize_pem().as_bytes())?;
    std::fs::write(dir.join("machine.crt"), &resp.cert_pem)?;
    std::fs::write(dir.join("ca.crt"), &resp.ca_pem)?;
    std::fs::write(dir.join("chain.crt"), format!("{}{}", resp.cert_pem, resp.ca_pem))?;
    #[cfg(target_os = "macos")]
    {
        // Import into the System keychain so apps and 802.1X can use it.
        for f in ["machine.key", "machine.crt"] {
            let p = dir.join(f);
            let _ = tokio::process::Command::new("security")
                .args(["import", &p.to_string_lossy(), "-k", "/Library/Keychains/System.keychain", "-A"])
                .output()
                .await;
        }
    }
    Ok(resp.not_after)
}
