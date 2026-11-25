//! Settings for the network services the server hosts.

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::auth::Admin;
use crate::error::{ApiError, ApiResult};
use crate::netsvc::{self, dns};
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
