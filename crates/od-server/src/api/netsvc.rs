//! Settings for the network services the server hosts.

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::auth::Admin;
use crate::error::{ApiError, ApiResult};
use crate::netsvc::{self, dhcp, dns, ldap, radius};
use crate::AppState;

#[derive(Serialize)]
pub struct DnsView {
    config: dns::DnsConfig,
    status: netsvc::ServiceStatus,
    zone: Option<String>,
    records: Vec<dns::Record>,
    /// Records generated from the server and endpoints (read-only).
    generated: Vec<dns::Record>,
}

pub async fn get_dns(State(s): State<AppState>, _a: Admin) -> ApiResult<Json<DnsView>> {
    let config = netsvc::dns_config(&s);
    let records: Vec<dns::Record> = s.db.meta_get("dns_records")?.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    let all = dns::zone_records(&s, &config);
    let generated = all.into_iter().filter(|r| r.id.is_nil()).collect();
    Ok(Json(DnsView { status: netsvc::dns_status(&s), zone: s.db.meta_get("domain")?, config, records, generated }))
}

pub async fn set_dns_config(
    State(s): State<AppState>,
    admin: Admin,
    Json(c): Json<dns::DnsConfig>,
) -> ApiResult<Json<netsvc::ServiceStatus>> {
    c.listen.parse::<std::net::SocketAddr>().map_err(|_| ApiError::bad_request("Listen on an address like 0.0.0.0:53"))?;
    for f in &c.forwarders {
        if f.parse::<std::net::IpAddr>().is_err() && f.parse::<std::net::SocketAddr>().is_err() {
            return Err(ApiError::bad_request(format!("\"{f}\" isn't an IP address")));
        }
    }
    s.db.meta_set("dns_config", &serde_json::to_string(&c).map_err(anyhow::Error::from)?)?;
    netsvc::restart_dns(&s);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    s.db.audit(admin.name(), "dns.config", "", if c.enabled { "DNS server on" } else { "DNS server off" });
    Ok(Json(netsvc::dns_status(&s)))
}

pub async fn set_dns_records(
    State(s): State<AppState>,
    admin: Admin,
    Json(mut recs): Json<Vec<dns::Record>>,
) -> ApiResult<Json<Vec<dns::Record>>> {
    if recs.len() > 5000 {
        return Err(ApiError::bad_request("At most 5000 records"));
    }
    for r in &mut recs {
        if dns::type_code(&r.rtype).is_none() {
            return Err(ApiError::bad_request(format!("Unsupported record type {}", r.rtype)));
        }
        r.rtype = r.rtype.to_ascii_uppercase();
        let name_ok = !r.name.is_empty() && r.name.len() <= 253 && r.name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.@*".contains(c));
        if !name_ok {
            return Err(ApiError::bad_request(format!("\"{}\" isn't a valid record name", r.name)));
        }
        match r.rtype.as_str() {
            "A" if r.value.parse::<std::net::Ipv4Addr>().is_err() => {
                return Err(ApiError::bad_request(format!("{}: A records need an IPv4 address", r.name)))
            }
            "AAAA" if r.value.parse::<std::net::Ipv6Addr>().is_err() => {
                return Err(ApiError::bad_request(format!("{}: AAAA records need an IPv6 address", r.name)))
            }
            _ => {}
        }
        if r.id.is_nil() {
            r.id = uuid::Uuid::new_v4();
        }
    }
    s.db.meta_set("dns_records", &serde_json::to_string(&recs).map_err(anyhow::Error::from)?)?;
    s.db.audit(admin.name(), "dns.records", "", &format!("{} record(s)", recs.len()));
    Ok(Json(recs))
}

#[derive(Serialize)]
pub struct DhcpView {
    config: dhcp::DhcpConfig,
    status: netsvc::ServiceStatus,
    leases: Vec<dhcp::Lease>,
    /// A guess for this server's address, for the setup form.
    suggested_server_ip: String,
}

pub async fn get_dhcp(State(s): State<AppState>, _a: Admin) -> ApiResult<Json<DhcpView>> {
    let now = chrono::Utc::now();
    let host = s.info.endpoint_url.split("://").nth(1).and_then(|h| h.rsplit_once(':').map(|(h, _)| h.to_string())).unwrap_or_default();
    Ok(Json(DhcpView {
        config: netsvc::dhcp_config(&s),
        status: netsvc::dhcp_status(&s),
        leases: dhcp::load_leases(&s).into_iter().filter(|l| l.expires > now && !l.offered).collect(),
        suggested_server_ip: host.parse::<std::net::Ipv4Addr>().map(|h| h.to_string()).unwrap_or_default(),
    }))
}

pub async fn set_dhcp(State(s): State<AppState>, admin: Admin, Json(c): Json<dhcp::DhcpConfig>) -> ApiResult<Json<netsvc::ServiceStatus>> {
    c.listen.parse::<std::net::SocketAddr>().map_err(|_| ApiError::bad_request("Listen on an address like 0.0.0.0:67"))?;
    dhcp::validate(&c).map_err(ApiError::bad_request)?;
    s.db.meta_set("dhcp_config", &serde_json::to_string(&c).map_err(anyhow::Error::from)?)?;
    netsvc::restart_dhcp(&s);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    s.db.audit(admin.name(), "dhcp.config", "", &format!("{} scope(s), {}", c.scopes.len(), if c.enabled { "on" } else { "off" }));
    Ok(Json(netsvc::dhcp_status(&s)))
}

#[derive(Serialize)]
pub struct RadiusView {
    config: radius::RadiusConfig,
    status: netsvc::ServiceStatus,
}

pub async fn get_radius(State(s): State<AppState>, _a: Admin) -> Json<RadiusView> {
    let mut config = netsvc::radius_config(&s);
    for c in &mut config.clients {
        c.secret = crate::destinations::KEEP.into();
    }
    Json(RadiusView { config, status: netsvc::radius_status(&s) })
}

pub async fn set_radius(
    State(s): State<AppState>,
    admin: Admin,
    Json(mut c): Json<radius::RadiusConfig>,
) -> ApiResult<Json<netsvc::ServiceStatus>> {
    c.listen.parse::<std::net::SocketAddr>().map_err(|_| ApiError::bad_request("Listen on an address like 0.0.0.0:1812"))?;
    let old = netsvc::radius_config(&s);
    for cl in &mut c.clients {
        if cl.secret == crate::destinations::KEEP {
            cl.secret = old.clients.iter().find(|o| o.name == cl.name).map(|o| o.secret.clone()).unwrap_or_default();
        }
        if cl.secret.len() < 12 {
            return Err(ApiError::bad_request(format!("{}: use a shared secret of at least 12 characters", cl.name)));
        }
        let ok = cl.address.parse::<std::net::IpAddr>().is_ok()
            || cl
                .address
                .split_once('/')
                .map(|(a, b)| a.parse::<std::net::Ipv4Addr>().is_ok() && b.parse::<u8>().map(|b| b <= 32).unwrap_or(false))
                .unwrap_or(false);
        if !ok {
            return Err(ApiError::bad_request(format!("{}: enter an IP address or a range like 10.0.0.0/24", cl.name)));
        }
    }
    netsvc::save_radius_config(&s, &c)?;
    netsvc::restart_radius(&s);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    s.db.audit(admin.name(), "radius.config", "", &format!("{} client(s), {}", c.clients.len(), if c.enabled { "on" } else { "off" }));
    Ok(Json(netsvc::radius_status(&s)))
}

#[derive(Serialize)]
pub struct LdapView {
    config: ldap::LdapConfig,
    status: netsvc::ServiceStatus,
    base_dn: String,
}

pub async fn get_ldap(State(s): State<AppState>, _a: Admin) -> ApiResult<Json<LdapView>> {
    Ok(Json(LdapView {
        config: netsvc::ldap_config(&s),
        status: netsvc::ldap_status(&s),
        base_dn: ldap::base_dn(&s.db.meta_get("domain")?.unwrap_or_default()),
    }))
}

pub async fn set_ldap(State(s): State<AppState>, admin: Admin, Json(c): Json<ldap::LdapConfig>) -> ApiResult<Json<netsvc::ServiceStatus>> {
    for a in [&c.listen, &c.ldaps_listen] {
        if !a.trim().is_empty() && a.parse::<std::net::SocketAddr>().is_err() {
            return Err(ApiError::bad_request(format!("\"{a}\" isn't an address like 0.0.0.0:636")));
        }
    }
    s.db.meta_set("ldap_config", &serde_json::to_string(&c).map_err(anyhow::Error::from)?)?;
    netsvc::restart_ldap(&s);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    s.db.audit(admin.name(), "ldap.config", "", if c.enabled { "LDAP server on" } else { "LDAP server off" });
    Ok(Json(netsvc::ldap_status(&s)))
}
