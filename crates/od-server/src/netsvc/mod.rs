//! Network services the Management Server can host: DNS (and later DHCP,
//! RADIUS). Each runs as a background task that can be restarted when its
//! settings change.

pub mod dhcp;
pub mod dns;
pub mod ldap;
pub mod radius;

use std::sync::Mutex;

use serde::Serialize;

use crate::AppState;

#[derive(Default)]
pub struct Services {
    dns: Mutex<Option<tokio::task::JoinHandle<()>>>,
    dns_status: Mutex<ServiceStatus>,
    dhcp: Mutex<Option<tokio::task::JoinHandle<()>>>,
    dhcp_status: Mutex<ServiceStatus>,
    radius: Mutex<Option<tokio::task::JoinHandle<()>>>,
    radius_status: Mutex<ServiceStatus>,
    ldap: Mutex<Option<tokio::task::JoinHandle<()>>>,
    ldap_status: Mutex<ServiceStatus>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ServiceStatus {
    pub running: bool,
    pub error: String,
}

pub fn dns_config(s: &AppState) -> dns::DnsConfig {
    s.db.meta_get("dns_config").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}

pub fn dns_status(s: &AppState) -> ServiceStatus {
    s.services.dns_status.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// (Re)start DNS from its saved settings.
pub fn restart_dns(s: &AppState) {
    if let Some(old) = s.services.dns.lock().unwrap_or_else(|e| e.into_inner()).take() {
        old.abort();
    }
    let cfg = dns_config(s);
    let set = |st: ServiceStatus| *s.services.dns_status.lock().unwrap_or_else(|e| e.into_inner()) = st;
    if !cfg.enabled {
        set(ServiceStatus::default());
        return;
    }
    set(ServiceStatus { running: true, error: String::new() });
    let s2 = s.clone();
    let task = tokio::spawn(async move {
        if let Err(e) = dns::serve(s2.clone(), cfg).await {
            tracing::warn!("DNS server stopped: {e:#}");
            *s2.services.dns_status.lock().unwrap_or_else(|e| e.into_inner()) = ServiceStatus { running: false, error: format!("{e:#}") };
        }
    });
    *s.services.dns.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
}

pub fn dhcp_config(s: &AppState) -> dhcp::DhcpConfig {
    s.db.meta_get("dhcp_config").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}

pub fn dhcp_status(s: &AppState) -> ServiceStatus {
    s.services.dhcp_status.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn restart_dhcp(s: &AppState) {
    if let Some(old) = s.services.dhcp.lock().unwrap_or_else(|e| e.into_inner()).take() {
        old.abort();
    }
    let cfg = dhcp_config(s);
    let set = |st: ServiceStatus| *s.services.dhcp_status.lock().unwrap_or_else(|e| e.into_inner()) = st;
    if !cfg.enabled {
        set(ServiceStatus::default());
        return;
    }
    set(ServiceStatus { running: true, error: String::new() });
    let s2 = s.clone();
    let task = tokio::spawn(async move {
        if let Err(e) = dhcp::serve(s2.clone(), cfg).await {
            tracing::warn!("DHCP server stopped: {e:#}");
            *s2.services.dhcp_status.lock().unwrap_or_else(|e| e.into_inner()) = ServiceStatus { running: false, error: format!("{e:#}") };
        }
    });
    *s.services.dhcp.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
}

/// RADIUS settings, sealed because they hold client shared secrets.
pub fn radius_config(s: &AppState) -> radius::RadiusConfig {
    s.db.meta_get("radius_config")
        .ok()
        .flatten()
        .and_then(|h| hex::decode(h).ok())
        .and_then(|b| s.sealer.open(&b).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
pub fn save_radius_config(s: &AppState, c: &radius::RadiusConfig) -> anyhow::Result<()> {
    s.db.meta_set("radius_config", &hex::encode(s.sealer.seal(&serde_json::to_vec(c)?)?))
}
pub fn radius_status(s: &AppState) -> ServiceStatus {
    s.services.radius_status.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn restart_radius(s: &AppState) {
    if let Some(old) = s.services.radius.lock().unwrap_or_else(|e| e.into_inner()).take() {
        old.abort();
    }
    let cfg = radius_config(s);
    let set = |st: ServiceStatus| *s.services.radius_status.lock().unwrap_or_else(|e| e.into_inner()) = st;
    if !cfg.enabled {
        set(ServiceStatus::default());
        return;
    }
    set(ServiceStatus { running: true, error: String::new() });
    let s2 = s.clone();
    let task = tokio::spawn(async move {
        if let Err(e) = radius::serve(s2.clone(), cfg).await {
            tracing::warn!("RADIUS server stopped: {e:#}");
            *s2.services.radius_status.lock().unwrap_or_else(|e| e.into_inner()) =
                ServiceStatus { running: false, error: format!("{e:#}") };
        }
    });
    *s.services.radius.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
}

pub fn ldap_config(s: &AppState) -> ldap::LdapConfig {
    s.db.meta_get("ldap_config").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}
pub fn ldap_status(s: &AppState) -> ServiceStatus {
    s.services.ldap_status.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
pub fn restart_ldap(s: &AppState) {
    if let Some(old) = s.services.ldap.lock().unwrap_or_else(|e| e.into_inner()).take() {
        old.abort();
    }
    let cfg = ldap_config(s);
    let set = |st: ServiceStatus| *s.services.ldap_status.lock().unwrap_or_else(|e| e.into_inner()) = st;
    if !cfg.enabled {
        set(ServiceStatus::default());
        return;
    }
    set(ServiceStatus { running: true, error: String::new() });
    let s2 = s.clone();
    let task = tokio::spawn(async move {
        if let Err(e) = ldap::serve(s2.clone(), cfg).await {
            tracing::warn!("LDAP server stopped: {e:#}");
            *s2.services.ldap_status.lock().unwrap_or_else(|e| e.into_inner()) = ServiceStatus { running: false, error: format!("{e:#}") };
        }
    });
    *s.services.ldap.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
}

pub fn start_all(s: &AppState) {
    restart_ldap(s);
    restart_dns(s);
    restart_dhcp(s);
    restart_radius(s);
}
