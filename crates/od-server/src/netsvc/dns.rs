//! A small authoritative DNS server for the domain's zone, with dynamic
//! records for enrolled endpoints (like AD-integrated DNS) and forwarding of
//! everything else to upstream resolvers.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DnsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_listen")]
    pub listen: String,
    /// Upstream resolvers for names outside the zone.
    #[serde(default = "default_forwarders")]
    pub forwarders: Vec<String>,
    /// Publish endpoint host names from their check-ins.
    #[serde(default = "yes")]
    pub dynamic_updates: bool,
    #[serde(default = "ttl")]
    pub default_ttl: u32,
}
fn default_listen() -> String {
    "0.0.0.0:53".into()
}
fn default_forwarders() -> Vec<String> {
    vec!["1.1.1.1".into(), "9.9.9.9".into()]
}
fn yes() -> bool {
    true
}
fn ttl() -> u32 {
    300
}
impl Default for DnsConfig {
    fn default() -> Self {
        DnsConfig { enabled: false, listen: default_listen(), forwarders: default_forwarders(), dynamic_updates: true, default_ttl: 300 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub id: uuid::Uuid,
    /// Relative to the zone ("@" for the zone itself), or a full name ending in ".".
    pub name: String,
    /// A, AAAA, CNAME, TXT, MX, SRV, PTR, NS
    #[serde(rename = "type")]
    pub rtype: String,
    pub value: String,
    #[serde(default)]
    pub ttl: Option<u32>,
    /// MX preference or SRV priority.
    #[serde(default)]
    pub priority: u16,
    #[serde(default)]
    pub weight: u16,
    #[serde(default)]
    pub port: u16,
}

pub fn type_code(t: &str) -> Option<u16> {
    Some(match t.to_ascii_uppercase().as_str() {
        "A" => 1,
        "NS" => 2,
        "CNAME" => 5,
        "SOA" => 6,
        "PTR" => 12,
        "MX" => 15,
        "TXT" => 16,
        "AAAA" => 28,
        "SRV" => 33,
        _ => return None,
    })
}

// ------------------------------------------------------------------ wire format

/// Read a (possibly compressed) name starting at `pos`. Returns (name, next position).
fn read_name(msg: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut labels = vec![];
    let mut jumped = false;
    let mut next = 0;
    for _ in 0..128 {
        let len = *msg.get(pos)? as usize;
        if len == 0 {
            if !jumped {
                next = pos + 1;
            }
            return Some((labels.join("."), next));
        }
        if len & 0xC0 == 0xC0 {
            let ptr = ((len & 0x3F) << 8) | *msg.get(pos + 1)? as usize;
            if !jumped {
                next = pos + 2;
            }
            jumped = true;
            pos = ptr;
            continue;
        }
        let label = msg.get(pos + 1..pos + 1 + len)?;
        labels.push(String::from_utf8_lossy(label).to_lowercase());
        pos += 1 + len;
    }
    None
}

fn write_name(out: &mut Vec<u8>, name: &str) {
    for l in name.trim_end_matches('.').split('.').filter(|l| !l.is_empty()) {
        let b = l.as_bytes();
        out.push(b.len().min(63) as u8);
        out.extend_from_slice(&b[..b.len().min(63)]);
    }
    out.push(0);
}

pub struct Question {
    pub id: u16,
    pub flags: u16,
    pub name: String,
    pub qtype: u16,
    pub qclass: u16,
    /// Bytes of the question section, copied into the answer.
    raw_question: Vec<u8>,
}

pub fn parse_question(msg: &[u8]) -> Option<Question> {
    if msg.len() < 12 || u16::from_be_bytes([msg[4], msg[5]]) != 1 {
        return None;
    }
    let (name, p) = read_name(msg, 12)?;
    let qtype = u16::from_be_bytes([*msg.get(p)?, *msg.get(p + 1)?]);
    let qclass = u16::from_be_bytes([*msg.get(p + 2)?, *msg.get(p + 3)?]);
    Some(Question {
        id: u16::from_be_bytes([msg[0], msg[1]]),
        flags: u16::from_be_bytes([msg[2], msg[3]]),
        name,
        qtype,
        qclass,
        raw_question: msg[12..p + 4].to_vec(),
    })
}

/// One answer: name, type and rdata.
pub struct Answer {
    pub name: String,
    pub rtype: u16,
    pub ttl: u32,
    pub rdata: Vec<u8>,
}

fn rdata(r: &Record, zone: &str) -> Option<Vec<u8>> {
    let fq = |v: &str| if v.ends_with('.') { v.to_string() } else { format!("{v}.{zone}") };
    let mut out = vec![];
    match r.rtype.to_ascii_uppercase().as_str() {
        "A" => out.extend(r.value.parse::<std::net::Ipv4Addr>().ok()?.octets()),
        "AAAA" => out.extend(r.value.parse::<std::net::Ipv6Addr>().ok()?.octets()),
        "CNAME" | "NS" | "PTR" => write_name(&mut out, &fq(&r.value)),
        "MX" => {
            out.extend(r.priority.to_be_bytes());
            write_name(&mut out, &fq(&r.value));
        }
        "SRV" => {
            out.extend(r.priority.to_be_bytes());
            out.extend(r.weight.to_be_bytes());
            out.extend(r.port.to_be_bytes());
            write_name(&mut out, &fq(&r.value));
        }
        "TXT" => {
            for chunk in r.value.as_bytes().chunks(255) {
                out.push(chunk.len() as u8);
                out.extend(chunk);
            }
        }
        _ => return None,
    }
    Some(out)
}

pub fn build_response(q: &Question, answers: &[Answer], rcode: u8, authoritative: bool, soa: Option<&Answer>) -> Vec<u8> {
    let mut out = Vec::with_capacity(512);
    out.extend(q.id.to_be_bytes());
    let rd = q.flags & 0x0100;
    let flags: u16 = 0x8000 | rd | 0x0080 | if authoritative { 0x0400 } else { 0 } | rcode as u16;
    out.extend(flags.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend((answers.len() as u16).to_be_bytes());
    out.extend((soa.is_some() as u16).to_be_bytes());
    out.extend(0u16.to_be_bytes());
    out.extend(&q.raw_question);
    for a in answers.iter().chain(soa) {
        write_name(&mut out, &a.name);
        out.extend(a.rtype.to_be_bytes());
        out.extend(1u16.to_be_bytes());
        out.extend(a.ttl.to_be_bytes());
        out.extend((a.rdata.len() as u16).to_be_bytes());
        out.extend(&a.rdata);
    }
    out
}

// ------------------------------------------------------------------ zone

/// Everything the server answers for: admin records plus generated ones.
pub fn zone_records(s: &AppState, cfg: &DnsConfig) -> Vec<Record> {
    let mut v: Vec<Record> = s.db.meta_get("dns_records").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    let server_ip = s
        .info
        .endpoint_url
        .split("://")
        .nth(1)
        .and_then(|h| h.rsplit_once(':').map(|(h, _)| h.trim_matches(['[', ']']).to_string()))
        .unwrap_or_default();
    let port = s.info.endpoint_url.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()).unwrap_or(8443);
    let mk = |name: &str, t: &str, value: String| Record {
        id: uuid::Uuid::nil(),
        name: name.into(),
        rtype: t.into(),
        value,
        ttl: None,
        priority: 0,
        weight: 0,
        port: 0,
    };
    if server_ip.parse::<IpAddr>().is_ok() {
        let t = if server_ip.contains(':') { "AAAA" } else { "A" };
        v.push(mk("od", t, server_ip.clone()));
        v.push(mk("@", "NS", "od".into()));
        v.push(Record { port, priority: 0, weight: 100, ..mk("_opendirectory._tcp", "SRV", "od".into()) });
    }
    if cfg.dynamic_updates {
        for e in s.db.list_endpoints().unwrap_or_default() {
            let host = e.hostname.to_lowercase().split('.').next().unwrap_or("").to_string();
            let ok = !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
            let ip = e
                .inventory
                .interfaces
                .iter()
                .flat_map(|i| &i.ips)
                .find(|ip| ip.parse::<std::net::Ipv4Addr>().map(|a| !a.is_loopback() && !a.is_link_local()).unwrap_or(false));
            if let (true, Some(ip)) = (ok, ip) {
                if !v.iter().any(|r| r.name.eq_ignore_ascii_case(&host)) {
                    v.push(mk(&host, "A", ip.clone()));
                }
            }
        }
    }
    v
}

/// Answer a question from the zone, or None if the name isn't in the zone.
pub fn answer(zone: &str, records: &[Record], q: &Question, default_ttl: u32) -> Option<(Vec<Answer>, u8, Answer)> {
    let zone = zone.trim_end_matches('.').to_lowercase();
    let name = q.name.trim_end_matches('.').to_lowercase();
    if name != zone && !name.ends_with(&format!(".{zone}")) {
        return None;
    }
    let rel = if name == zone { "@".to_string() } else { name[..name.len() - zone.len() - 1].to_string() };
    let soa = {
        let mut d = vec![];
        write_name(&mut d, &format!("od.{zone}"));
        write_name(&mut d, &format!("hostmaster.{zone}"));
        for n in [1u32, 3600, 600, 86400, 60] {
            d.extend(n.to_be_bytes());
        }
        Answer { name: zone.clone(), rtype: 6, ttl: 60, rdata: d }
    };
    let matching: Vec<&Record> = records
        .iter()
        .filter(|r| r.name.trim_end_matches('.').eq_ignore_ascii_case(&rel) || r.name.trim_end_matches('.').eq_ignore_ascii_case(&name))
        .collect();
    if matching.is_empty() {
        return Some((vec![], 3, soa)); // NXDOMAIN
    }
    let mut out = vec![];
    for r in &matching {
        let Some(t) = type_code(&r.rtype) else { continue };
        // A CNAME answers every type.
        if t == q.qtype || q.qtype == 255 || t == 5 {
            if let Some(rd) = rdata(r, &zone) {
                out.push(Answer { name: name.clone(), rtype: t, ttl: r.ttl.unwrap_or(default_ttl), rdata: rd });
            }
        }
    }
    if q.qtype == 6 && rel == "@" {
        return Some((vec![Answer { name: soa.name.clone(), rtype: 6, ttl: soa.ttl, rdata: soa.rdata.clone() }], 0, soa));
    }
    Some((out, 0, soa))
}

async fn forward(cfg: &DnsConfig, packet: &[u8]) -> Option<Vec<u8>> {
    for f in &cfg.forwarders {
        let addr: SocketAddr =
            if f.contains(':') && !f.contains('.') || f.contains("]:") { f.parse().ok()? } else { format!("{f}:53").parse().ok()? };
        let sock = UdpSocket::bind(if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).await.ok()?;
        if sock.send_to(packet, addr).await.is_err() {
            continue;
        }
        let mut buf = vec![0u8; 4096];
        if let Ok(Ok((n, _))) = tokio::time::timeout(Duration::from_secs(2), sock.recv_from(&mut buf)).await {
            buf.truncate(n);
            return Some(buf);
        }
    }
    None
}

/// Handle one query packet.
pub async fn handle(s: &AppState, cfg: &DnsConfig, packet: &[u8]) -> Option<Vec<u8>> {
    let q = parse_question(packet)?;
    let zone = s.db.meta_get("domain").ok().flatten().unwrap_or_default();
    if !zone.is_empty() {
        let recs = zone_records(s, cfg);
        if let Some((answers, rcode, soa)) = answer(&zone, &recs, &q, cfg.default_ttl) {
            let soa_ref = if answers.is_empty() { Some(&soa) } else { None };
            return Some(build_response(&q, &answers, rcode, true, soa_ref));
        }
    }
    match forward(cfg, packet).await {
        Some(r) => Some(r),
        None => Some(build_response(&q, &[], 2, false, None)), // SERVFAIL
    }
}

/// Run the DNS server until the task is aborted.
pub async fn serve(s: AppState, cfg: DnsConfig) -> Result<()> {
    let addr: SocketAddr = cfg.listen.parse().context("DNS listen address")?;
    let udp = std::sync::Arc::new(
        UdpSocket::bind(addr).await.with_context(|| format!("bind DNS on UDP {addr} (port 53 needs administrator/root rights)"))?,
    );
    let tcp = TcpListener::bind(addr).await.with_context(|| format!("bind DNS on TCP {addr}"))?;
    tracing::info!("DNS server listening on {addr}");
    let (s2, cfg2) = (s.clone(), cfg.clone());
    let tcp_task = tokio::spawn(async move {
        while let Ok((mut c, _)) = tcp.accept().await {
            let (s, cfg) = (s2.clone(), cfg2.clone());
            tokio::spawn(async move {
                let mut len = [0u8; 2];
                while c.read_exact(&mut len).await.is_ok() {
                    let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
                    if c.read_exact(&mut buf).await.is_err() {
                        break;
                    }
                    let Some(resp) = handle(&s, &cfg, &buf).await else { break };
                    let mut out = (resp.len() as u16).to_be_bytes().to_vec();
                    out.extend(resp);
                    if c.write_all(&out).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    let mut buf = vec![0u8; 4096];
    loop {
        let (n, peer) = match udp.recv_from(&mut buf).await {
            Ok(x) => x,
            Err(_) => continue,
        };
        let packet = buf[..n].to_vec();
        let (s, cfg, udp) = (s.clone(), cfg.clone(), udp.clone());
        tokio::spawn(async move {
            if let Some(resp) = handle(&s, &cfg, &packet).await {
                let resp = if resp.len() > 512 { truncate(resp) } else { resp };
                let _ = udp.send_to(&resp, peer).await;
            }
        });
        if tcp_task.is_finished() {
            break;
        }
    }
    Ok(())
}

/// Too big for UDP: set the TC bit and drop the records so the client retries over TCP.
fn truncate(mut r: Vec<u8>) -> Vec<u8> {
    if r.len() < 12 {
        return r;
    }
    r[2] |= 0x02;
    for b in &mut r[6..12] {
        *b = 0;
    }
    let qend = read_name(&r, 12).map(|(_, p)| p + 4).unwrap_or(r.len());
    r.truncate(qend);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut m = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        write_name(&mut m, name);
        m.extend(qtype.to_be_bytes());
        m.extend(1u16.to_be_bytes());
        m
    }

    #[test]
    fn answers_from_the_zone() {
        let recs = vec![
            Record {
                id: uuid::Uuid::nil(),
                name: "pc-01".into(),
                rtype: "A".into(),
                value: "10.0.0.21".into(),
                ttl: None,
                priority: 0,
                weight: 0,
                port: 0,
            },
            Record {
                id: uuid::Uuid::nil(),
                name: "www".into(),
                rtype: "CNAME".into(),
                value: "pc-01".into(),
                ttl: Some(60),
                priority: 0,
                weight: 0,
                port: 0,
            },
            Record {
                id: uuid::Uuid::nil(),
                name: "_ldap._tcp".into(),
                rtype: "SRV".into(),
                value: "od".into(),
                ttl: None,
                priority: 0,
                weight: 100,
                port: 389,
            },
        ];
        let q = parse_question(&query("PC-01.corp.example.com", 1)).unwrap();
        assert_eq!(q.name, "pc-01.corp.example.com");
        let (a, rcode, _) = answer("corp.example.com", &recs, &q, 300).unwrap();
        assert_eq!((a.len(), rcode), (1, 0));
        assert_eq!(a[0].rdata, vec![10, 0, 0, 21]);
        let resp = build_response(&q, &a, rcode, true, None);
        assert_eq!(&resp[..2], &[0x12, 0x34]);
        assert_eq!(u16::from_be_bytes([resp[6], resp[7]]), 1, "one answer");
        assert!(resp[2] & 0x04 != 0, "authoritative");

        let q = parse_question(&query("www.corp.example.com", 1)).unwrap();
        assert_eq!(answer("corp.example.com", &recs, &q, 300).unwrap().0[0].rtype, 5);
        let q = parse_question(&query("nope.corp.example.com", 1)).unwrap();
        assert_eq!(answer("corp.example.com", &recs, &q, 300).unwrap().1, 3, "NXDOMAIN");
        let q = parse_question(&query("example.org", 1)).unwrap();
        assert!(answer("corp.example.com", &recs, &q, 300).is_none(), "outside the zone");
        let q = parse_question(&query("_ldap._tcp.corp.example.com", 33)).unwrap();
        assert_eq!(answer("corp.example.com", &recs, &q, 300).unwrap().0.len(), 1);
    }

    #[test]
    fn compressed_names_parse() {
        let mut m = vec![0; 12];
        write_name(&mut m, "a.example.com");
        let ptr_at = m.len();
        m.extend([1, b'b', 0xC0, 14]); // "b" + pointer to "example.com"
        let (n, next) = read_name(&m, ptr_at).unwrap();
        assert_eq!(n, "b.example.com");
        assert_eq!(next, ptr_at + 4);
    }
}
