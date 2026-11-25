//! Network services the Management Server can host: DNS (and later DHCP,
//! RADIUS). Each runs as a background task that can be restarted when its
//! settings change.

pub mod dhcp;
pub mod dns;

use std::sync::Mutex;

use serde::Serialize;

use crate::AppState;

#[derive(Default)]
pub struct Services {
    dns: Mutex<Option<tokio::task::JoinHandle<()>>>,
    dns_status: Mutex<ServiceStatus>,
    dhcp: Mutex<Option<tokio::task::JoinHandle<()>>>,
    dhcp_status: Mutex<ServiceStatus>,
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

pub fn start_all(s: &AppState) {
    restart_dns(s);
    restart_dhcp(s);
}
