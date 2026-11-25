//! Mobile devices: an inventory of phones and tablets (serial, IMEI, owner,
//! IP, status) and Apple configuration profiles (passcode, Wi-Fi, trusted CA,
//! restrictions) that can be installed without an MDM server.
//!
//! Full MDM enrollment needs an Apple Push certificate (Apple Business
//! Manager) and Android Enterprise registration with Google; those aren't
//! included yet.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::Admin;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobileDevice {
    #[serde(default)]
    pub id: Uuid,
    pub name: String,
    /// ios, ipados, android or other
    pub platform: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub serial: String,
    #[serde(default)]
    pub imei: String,
    #[serde(default)]
    pub phone_number: String,
    #[serde(default)]
    pub owner_user_id: Option<Uuid>,
    /// corporate or byod
    #[serde(default = "corporate")]
    pub ownership: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub mac: String,
    #[serde(default)]
    pub os_version: String,
    /// active, lost or retired
    #[serde(default = "active")]
    pub status: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}
fn corporate() -> String {
    "corporate".into()
}
fn active() -> String {
    "active".into()
}

fn all(s: &AppState) -> Vec<MobileDevice> {
    s.db.meta_get("mobile_devices").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}
fn save(s: &AppState, v: &[MobileDevice]) -> ApiResult<()> {
    s.db.meta_set("mobile_devices", &serde_json::to_string(v).map_err(anyhow::Error::from)?)?;
    Ok(())
}

fn check(d: &mut MobileDevice) -> ApiResult<()> {
    d.name = super::required("Name", &d.name)?;
    if !["ios", "ipados", "android", "other"].contains(&d.platform.as_str()) {
        return Err(ApiError::bad_request("Platform must be ios, ipados, android or other"));
    }
    if !["corporate", "byod"].contains(&d.ownership.as_str()) || !["active", "lost", "retired"].contains(&d.status.as_str()) {
        return Err(ApiError::bad_request("Ownership is corporate or byod; status is active, lost or retired"));
    }
    let imei: String = d.imei.chars().filter(|c| c.is_ascii_digit()).collect();
    if !d.imei.is_empty() && imei.len() != 15 {
        return Err(ApiError::bad_request(format!("{}: an IMEI has 15 digits", d.name)));
    }
    d.imei = imei;
    if !d.ip.is_empty() && d.ip.parse::<std::net::IpAddr>().is_err() {
        return Err(ApiError::bad_request(format!("{}: \"{}\" isn't an IP address", d.name, d.ip)));
    }
    d.updated_at = Some(Utc::now());
    Ok(())
}

pub async fn list(State(s): State<AppState>, _a: Admin) -> Json<Vec<MobileDevice>> {
    Json(all(&s))
}

/// Add one device, or several at once (CSV import).
pub async fn create(
    State(s): State<AppState>,
    admin: Admin,
    Json(mut devs): Json<Vec<MobileDevice>>,
) -> ApiResult<(StatusCode, Json<Vec<MobileDevice>>)> {
    let mut list = all(&s);
    for d in &mut devs {
        d.id = Uuid::new_v4();
        check(d)?;
        if !d.serial.is_empty() && list.iter().any(|x| x.serial.eq_ignore_ascii_case(&d.serial)) {
            return Err(ApiError::conflict(format!("A device with serial {} already exists", d.serial)));
        }
    }
    list.extend(devs.iter().cloned());
    save(&s, &list)?;
    s.db.audit(admin.name(), "mobile.add", &format!("{} device(s)", devs.len()), "");
    Ok((StatusCode::CREATED, Json(devs)))
}

pub async fn update(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(mut d): Json<MobileDevice>,
) -> ApiResult<Json<MobileDevice>> {
    let mut list = all(&s);
    let i = list.iter().position(|x| x.id == id).ok_or_else(|| ApiError::not_found("Mobile device"))?;
    d.id = id;
    check(&mut d)?;
    let was = list[i].status.clone();
    list[i] = d.clone();
    save(&s, &list)?;
    s.db.audit(admin.name(), if d.status == "lost" && was != "lost" { "mobile.lost" } else { "mobile.update" }, &d.name, &d.status);
    Ok(Json(d))
}

pub async fn remove(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let mut list = all(&s);
    let name = list.iter().find(|x| x.id == id).map(|x| x.name.clone()).ok_or_else(|| ApiError::not_found("Mobile device"))?;
    list.retain(|x| x.id != id);
    save(&s, &list)?;
    s.db.audit(admin.name(), "mobile.delete", &name, "");
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------------ Apple profiles

#[derive(Deserialize, Default)]
pub struct Passcode {
    #[serde(default = "six")]
    min_length: u32,
    #[serde(default)]
    require_alphanumeric: bool,
    /// Auto-lock after this many minutes (1–15).
    #[serde(default = "five")]
    max_inactivity_minutes: u32,
    /// Erase after this many failed attempts (0 = never).
    #[serde(default)]
    max_failed_attempts: u32,
}
fn six() -> u32 {
    6
}
fn five() -> u32 {
    5
}

#[derive(Deserialize)]
pub struct Wifi {
    ssid: String,
    /// WPA2, WPA3, or Enterprise (802.1X; the user signs in with directory credentials).
    #[serde(default = "wpa2")]
    security: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    hidden: bool,
}
fn wpa2() -> String {
    "WPA2".into()
}

#[derive(Deserialize, Default)]
pub struct Restrictions {
    #[serde(default = "yes")]
    allow_camera: bool,
    #[serde(default = "yes")]
    allow_screenshots: bool,
    #[serde(default = "yes")]
    allow_icloud_backup: bool,
    #[serde(default = "yes")]
    allow_app_installs: bool,
}
fn yes() -> bool {
    true
}

#[derive(Deserialize)]
pub struct ProfileInput {
    name: String,
    #[serde(default)]
    passcode: Option<Passcode>,
    #[serde(default)]
    wifi: Vec<Wifi>,
    /// Include the domain's root CA (Network Services > Certificate Authority).
    #[serde(default)]
    trust_ca: bool,
    #[serde(default)]
    restrictions: Option<Restrictions>,
}

fn x(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn payload(kind: &str, ident: &str, display: &str, body: &str) -> String {
    format!(
        "<dict><key>PayloadType</key><string>{kind}</string><key>PayloadVersion</key><integer>1</integer><key>PayloadIdentifier</key><string>{}</string><key>PayloadUUID</key><string>{}</string><key>PayloadDisplayName</key><string>{}</string>{body}</dict>",
        x(ident),
        Uuid::new_v4().to_string().to_uppercase(),
        x(display)
    )
}

/// Build an (unsigned) Apple configuration profile.
pub async fn apple_profile(State(s): State<AppState>, admin: Admin, Json(inp): Json<ProfileInput>) -> ApiResult<Json<serde_json::Value>> {
    let org = s.db.meta_get("organization")?.unwrap_or_else(|| "OpenDirectory".into());
    let domain = s.db.meta_get("domain")?.unwrap_or_else(|| "opendirectory.local".into());
    let base: String = domain.split('.').rev().collect::<Vec<_>>().join(".");
    let mut payloads = vec![];
    if let Some(p) = &inp.passcode {
        payloads.push(payload(
            "com.apple.mobiledevice.passwordpolicy",
            &format!("{base}.passcode"),
            "Passcode",
            &format!(
                "<key>forcePIN</key><true/><key>allowSimple</key><false/><key>minLength</key><integer>{}</integer><key>requireAlphanumeric</key><{}/><key>maxInactivity</key><integer>{}</integer>{}",
                p.min_length.clamp(4, 16),
                p.require_alphanumeric,
                p.max_inactivity_minutes.clamp(1, 15),
                if p.max_failed_attempts > 0 { format!("<key>maxFailedAttempts</key><integer>{}</integer>", p.max_failed_attempts.clamp(2, 11)) } else { String::new() }
            ),
        ));
    }
    for (i, w) in inp.wifi.iter().enumerate() {
        if w.ssid.trim().is_empty() {
            return Err(ApiError::bad_request("Each Wi-Fi network needs an SSID"));
        }
        let (enc, extra) = match w.security.as_str() {
            "Enterprise" => ("WPA2", "<key>EAPClientConfiguration</key><dict><key>AcceptEAPTypes</key><array><integer>25</integer></array><key>TTLSInnerAuthentication</key><string>MSCHAPv2</string></dict>".to_string()),
            "WPA3" => ("WPA3", format!("<key>Password</key><string>{}</string>", x(&w.password))),
            _ => ("WPA2", format!("<key>Password</key><string>{}</string>", x(&w.password))),
        };
        payloads.push(payload(
            "com.apple.wifi.managed",
            &format!("{base}.wifi.{i}"),
            &format!("Wi-Fi {}", w.ssid),
            &format!("<key>SSID_STR</key><string>{}</string><key>EncryptionType</key><string>{enc}</string><key>HIDDEN_NETWORK</key><{}/><key>AutoJoin</key><true/>{extra}", x(&w.ssid), w.hidden),
        ));
    }
    if inp.trust_ca {
        let ca: serde_json::Value =
            s.db.meta_get("ca")?
                .and_then(|j| serde_json::from_str(&j).ok())
                .ok_or_else(|| ApiError::bad_request("Create the certificate authority first (Network Services)"))?;
        let pem = ca["cert_pem"].as_str().unwrap_or("");
        let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
        let der = base64::engine::general_purpose::STANDARD.decode(&b64).map_err(anyhow::Error::from)?;
        payloads.push(payload(
            "com.apple.security.root",
            &format!("{base}.rootca"),
            ca["common_name"].as_str().unwrap_or("Root CA"),
            &format!("<key>PayloadContent</key><data>{}</data>", base64::engine::general_purpose::STANDARD.encode(der)),
        ));
    }
    if let Some(r) = &inp.restrictions {
        payloads.push(payload(
            "com.apple.applicationaccess",
            &format!("{base}.restrictions"),
            "Restrictions",
            &format!(
                "<key>allowCamera</key><{}/><key>allowScreenShot</key><{}/><key>allowCloudBackup</key><{}/><key>allowAppInstallation</key><{}/>",
                r.allow_camera, r.allow_screenshots, r.allow_icloud_backup, r.allow_app_installs
            ),
        ));
    }
    if payloads.is_empty() {
        return Err(ApiError::bad_request("Choose at least one thing for the profile to configure"));
    }
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>PayloadType</key><string>Configuration</string><key>PayloadVersion</key><integer>1</integer><key>PayloadIdentifier</key><string>{base}.profile.{}</string><key>PayloadUUID</key><string>{}</string><key>PayloadDisplayName</key><string>{}</string><key>PayloadOrganization</key><string>{}</string><key>PayloadDescription</key><string>Settings from your IT team, created with OpenDirectory.</string><key>PayloadContent</key><array>{}</array></dict></plist>
"#,
        Uuid::new_v4().simple(),
        Uuid::new_v4().to_string().to_uppercase(),
        x(&inp.name),
        x(&org),
        payloads.join("")
    );
    s.db.audit(admin.name(), "mobile.profile", &inp.name, &format!("{} payload(s)", payloads.len()));
    let file: String = inp.name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    Ok(Json(serde_json::json!({ "file_name": format!("{file}.mobileconfig"), "content": xml })))
}
