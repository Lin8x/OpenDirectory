//! A basic internal certificate authority (like a standalone AD CS root):
//! create the domain's root CA, issue server and client certificates (or
//! sign CSRs), revoke them and publish a CRL. Endpoints trust the root
//! through the "Trusted root certificate" GPO setting.

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use rcgen::{
    BasicConstraints, CertificateParams, CertificateRevocationListParams, CertificateSigningRequestParams, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, RevokedCertParams, SanType, SerialNumber,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::Admin;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

#[derive(Serialize, Deserialize, Clone)]
struct CaMeta {
    common_name: String,
    organization: String,
    cert_pem: String,
    created_at: DateTime<Utc>,
    not_after: DateTime<Utc>,
    crl_number: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Issued {
    pub id: Uuid,
    pub serial: String,
    pub common_name: String,
    pub sans: Vec<String>,
    pub kind: String,
    pub not_after: DateTime<Utc>,
    pub issued_at: DateTime<Utc>,
    pub issued_by: String,
    #[serde(default)]
    pub revoked_at: Option<DateTime<Utc>>,
    pub cert_pem: String,
}

fn to_time(t: DateTime<Utc>) -> time::OffsetDateTime {
    time::OffsetDateTime::from_unix_timestamp(t.timestamp()).unwrap_or(time::OffsetDateTime::now_utc())
}

fn meta(s: &AppState) -> Option<CaMeta> {
    s.db.meta_get("ca").ok().flatten().and_then(|j| serde_json::from_str(&j).ok())
}
fn issued(s: &AppState) -> Vec<Issued> {
    s.db.meta_get("ca_issued").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}
fn save_issued(s: &AppState, v: &[Issued]) -> ApiResult<()> {
    s.db.meta_set("ca_issued", &serde_json::to_string(v).map_err(anyhow::Error::from)?)?;
    Ok(())
}

fn ca_params(m: &CaMeta) -> CertificateParams {
    let mut p = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, m.common_name.clone());
    dn.push(DnType::OrganizationName, m.organization.clone());
    p.distinguished_name = dn;
    p.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    p.not_before = to_time(m.created_at);
    p.not_after = to_time(m.not_after);
    p
}

/// The CA's certificate and key, ready to sign. Re-creating the certificate
/// from the same name and key gives the same issuer for chaining.
fn signer(s: &AppState) -> ApiResult<(rcgen::Certificate, KeyPair, CaMeta)> {
    let m = meta(s).ok_or_else(|| ApiError::bad_request("Create the certificate authority first"))?;
    let sealed = s.db.meta_get("ca_key")?.ok_or_else(|| ApiError::bad_request("The CA key is missing"))?;
    let key_pem = String::from_utf8(s.sealer.open(&hex::decode(sealed).map_err(anyhow::Error::from)?)?).map_err(anyhow::Error::from)?;
    let kp = KeyPair::from_pem(&key_pem).map_err(|e| anyhow::anyhow!("CA key: {e}"))?;
    let cert = ca_params(&m).self_signed(&kp).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok((cert, kp, m))
}

#[derive(Serialize)]
pub struct CaView {
    initialized: bool,
    common_name: Option<String>,
    not_after: Option<DateTime<Utc>>,
    root_pem: Option<String>,
    issued: Vec<Issued>,
}

pub async fn get(State(s): State<AppState>, _a: Admin) -> Json<CaView> {
    let m = meta(&s);
    let mut list = issued(&s);
    list.iter_mut().for_each(|i| i.cert_pem.clear());
    Json(CaView {
        initialized: m.is_some(),
        common_name: m.as_ref().map(|m| m.common_name.clone()),
        not_after: m.as_ref().map(|m| m.not_after),
        root_pem: m.map(|m| m.cert_pem),
        issued: list,
    })
}

#[derive(Deserialize)]
pub struct InitInput {
    #[serde(default)]
    common_name: String,
    #[serde(default = "ten")]
    years: u32,
}
fn ten() -> u32 {
    10
}

pub async fn init(State(s): State<AppState>, admin: Admin, Json(inp): Json<InitInput>) -> ApiResult<(StatusCode, Json<CaView>)> {
    if meta(&s).is_some() {
        return Err(ApiError::conflict("This domain already has a certificate authority"));
    }
    let org = s.db.meta_get("organization")?.unwrap_or_else(|| "OpenDirectory".into());
    let domain = s.db.meta_get("domain")?.unwrap_or_default();
    let cn = if inp.common_name.trim().is_empty() { format!("{org} Root CA") } else { inp.common_name.trim().to_string() };
    let now = Utc::now();
    let m = CaMeta {
        common_name: cn,
        organization: org,
        cert_pem: String::new(),
        created_at: now,
        not_after: now + chrono::Duration::days(365 * inp.years.clamp(1, 30) as i64),
        crl_number: 1,
    };
    let kp = KeyPair::generate().map_err(|e| anyhow::anyhow!("{e}"))?;
    let cert = ca_params(&m).self_signed(&kp).map_err(|e| anyhow::anyhow!("{e}"))?;
    let m = CaMeta { cert_pem: cert.pem(), ..m };
    s.db.meta_set("ca_key", &hex::encode(s.sealer.seal(kp.serialize_pem().as_bytes())?))?;
    s.db.meta_set("ca", &serde_json::to_string(&m).map_err(anyhow::Error::from)?)?;
    s.db.audit(admin.name(), "ca.create", &m.common_name, &format!("for {domain}, valid {} years", inp.years));
    Ok((StatusCode::CREATED, get(State(s), admin).await))
}

#[derive(Deserialize)]
pub struct IssueInput {
    /// "server" (TLS server) or "client" (TLS client / user).
    #[serde(default = "server")]
    kind: String,
    #[serde(default)]
    common_name: String,
    #[serde(default)]
    sans: Vec<String>,
    #[serde(default = "year")]
    days: u32,
    /// Sign this CSR instead of generating a key.
    #[serde(default)]
    csr_pem: String,
}
fn server() -> String {
    "server".into()
}
fn year() -> u32 {
    397
}

#[derive(Serialize)]
pub struct IssueResult {
    certificate: Issued,
    /// Only when the server generated the key. Shown once; not stored.
    private_key_pem: Option<String>,
    chain_pem: String,
}

pub async fn issue(State(s): State<AppState>, admin: Admin, Json(inp): Json<IssueInput>) -> ApiResult<(StatusCode, Json<IssueResult>)> {
    let (ca, ca_key, m) = signer(&s)?;
    let now = Utc::now();
    let days = inp.days.clamp(1, 825);
    let mut serial = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut serial);
    serial[0] &= 0x7f;
    let usages = |p: &mut CertificateParams| {
        p.not_before = to_time(now - chrono::Duration::minutes(5));
        p.not_after = to_time((now + chrono::Duration::days(days as i64)).min(m.not_after));
        p.serial_number = Some(SerialNumber::from_slice(&serial));
        p.key_usages = vec![KeyUsagePurpose::DigitalSignature, KeyUsagePurpose::KeyEncipherment];
        p.extended_key_usages = if inp.kind == "client" {
            vec![ExtendedKeyUsagePurpose::ClientAuth]
        } else {
            vec![ExtendedKeyUsagePurpose::ServerAuth, ExtendedKeyUsagePurpose::ClientAuth]
        };
        p.is_ca = IsCa::ExplicitNoCa;
    };
    let (cert_pem, key_pem, cn, sans, not_after) = if !inp.csr_pem.trim().is_empty() {
        let mut csr = CertificateSigningRequestParams::from_pem(inp.csr_pem.trim())
            .map_err(|e| ApiError::bad_request(format!("That CSR couldn't be read: {e}")))?;
        usages(&mut csr.params);
        let cn = csr.params.distinguished_name.get(&DnType::CommonName).map(|v| format!("{v:?}")).unwrap_or_default();
        let sans: Vec<String> = csr.params.subject_alt_names.iter().map(|s| format!("{s:?}")).collect();
        let na = csr.params.not_after;
        let c = csr.signed_by(&ca, &ca_key).map_err(|e| ApiError::bad_request(format!("{e}")))?;
        (c.pem(), None, cn, sans, na)
    } else {
        let cn = inp.common_name.trim().to_string();
        if cn.is_empty() {
            return Err(ApiError::bad_request("Enter a common name, e.g. intranet.corp.example.com"));
        }
        let mut names: Vec<String> = inp.sans.iter().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
        if inp.kind == "server" && !names.contains(&cn) {
            names.insert(0, cn.clone());
        }
        let mut p = CertificateParams::new(Vec::<String>::new()).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, cn.clone());
        dn.push(DnType::OrganizationName, m.organization.clone());
        p.distinguished_name = dn;
        for n in &names {
            p.subject_alt_names.push(match n.parse::<std::net::IpAddr>() {
                Ok(ip) => SanType::IpAddress(ip),
                Err(_) if n.contains('@') => {
                    SanType::Rfc822Name(n.clone().try_into().map_err(|_| ApiError::bad_request(format!("Invalid name {n}")))?)
                }
                Err(_) => SanType::DnsName(n.clone().try_into().map_err(|_| ApiError::bad_request(format!("Invalid name {n}")))?),
            });
        }
        usages(&mut p);
        let na = p.not_after;
        let kp = KeyPair::generate().map_err(|e| anyhow::anyhow!("{e}"))?;
        let c = p.signed_by(&kp, &ca, &ca_key).map_err(|e| anyhow::anyhow!("{e}"))?;
        (c.pem(), Some(kp.serialize_pem()), cn, names, na)
    };
    let cert = Issued {
        id: Uuid::new_v4(),
        serial: hex::encode(serial),
        common_name: cn,
        sans,
        kind: inp.kind.clone(),
        not_after: DateTime::from_timestamp(not_after.unix_timestamp(), 0).unwrap_or(now),
        issued_at: now,
        issued_by: admin.name().to_string(),
        revoked_at: None,
        cert_pem,
    };
    let mut all = issued(&s);
    all.insert(0, cert.clone());
    save_issued(&s, &all)?;
    s.db.audit(admin.name(), "ca.issue", &cert.common_name, &format!("serial {}", cert.serial));
    Ok((
        StatusCode::CREATED,
        Json(IssueResult { chain_pem: format!("{}{}", cert.cert_pem, m.cert_pem), certificate: cert, private_key_pem: key_pem }),
    ))
}

pub async fn download(State(s): State<AppState>, _a: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<Issued>> {
    issued(&s).into_iter().find(|i| i.id == id).map(Json).ok_or_else(|| ApiError::not_found("Certificate"))
}

pub async fn revoke(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let mut all = issued(&s);
    let c = all.iter_mut().find(|i| i.id == id).ok_or_else(|| ApiError::not_found("Certificate"))?;
    if c.revoked_at.is_none() {
        c.revoked_at = Some(Utc::now());
    }
    let name = c.common_name.clone();
    save_issued(&s, &all)?;
    s.db.audit(admin.name(), "ca.revoke", &name, "");
    Ok(StatusCode::NO_CONTENT)
}

/// Public: the root certificate, for installing by hand.
pub async fn root_pem(State(s): State<AppState>) -> ApiResult<impl IntoResponse> {
    let m = meta(&s).ok_or_else(|| ApiError::not_found("Certificate authority"))?;
    Ok(([(header::CONTENT_TYPE, "application/x-pem-file")], m.cert_pem))
}

/// Public: the current certificate revocation list (PEM).
pub async fn crl(State(s): State<AppState>) -> ApiResult<impl IntoResponse> {
    let (ca, key, mut m) = signer(&s)?;
    let now = Utc::now();
    let revoked = issued(&s)
        .into_iter()
        .filter_map(|i| {
            Some(RevokedCertParams {
                serial_number: SerialNumber::from_slice(&hex::decode(&i.serial).ok()?),
                revocation_time: to_time(i.revoked_at?),
                reason_code: None,
                invalidity_date: None,
            })
        })
        .collect();
    m.crl_number += 1;
    let params = CertificateRevocationListParams {
        this_update: to_time(now),
        next_update: to_time(now + chrono::Duration::days(7)),
        crl_number: SerialNumber::from(m.crl_number),
        issuing_distribution_point: None,
        revoked_certs: revoked,
        key_identifier_method: rcgen::KeyIdMethod::Sha256,
    };
    let crl = params.signed_by(&ca, &key).map_err(|e| anyhow::anyhow!("{e}"))?;
    s.db.meta_set("ca", &serde_json::to_string(&m).map_err(anyhow::Error::from)?)?;
    Ok(([(header::CONTENT_TYPE, "application/x-pem-file")], crl.pem().map_err(|e| anyhow::anyhow!("{e}"))?))
}
