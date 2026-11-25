//! A DHCPv4 server: scopes with address ranges, gateway, DNS and lease time,
//! reservations by MAC address, relay (giaddr) support, and leases that also
//! feed DNS. Off by default: two DHCP servers on one network cause outages.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Reservation {
    pub mac: String,
    pub ip: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scope {
    pub name: String,
    /// e.g. 10.0.20.0/24
    pub network: String,
    pub range_start: String,
    pub range_end: String,
    #[serde(default)]
    pub gateway: String,
    #[serde(default)]
    pub dns: Vec<String>,
    #[serde(default)]
    pub domain: String,
    #[serde(default = "eight")]
    pub lease_hours: u32,
    #[serde(default)]
    pub reservations: Vec<Reservation>,
}
fn eight() -> u32 {
    8
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DhcpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "listen")]
    pub listen: String,
    /// This server's address on the network it serves (the DHCP server identifier).
    #[serde(default)]
    pub server_ip: String,
    #[serde(default)]
    pub scopes: Vec<Scope>,
}
fn listen() -> String {
    "0.0.0.0:67".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lease {
    pub mac: String,
    pub ip: String,
    #[serde(default)]
    pub hostname: String,
    pub expires: DateTime<Utc>,
    pub scope: String,
    /// An offer not yet accepted.
    #[serde(default)]
    pub offered: bool,
}

fn ip(s: &str) -> Option<Ipv4Addr> {
    s.trim().parse().ok()
}
fn cidr(s: &str) -> Option<(u32, u32)> {
    let (a, b) = s.split_once('/')?;
    let bits: u32 = b.parse().ok()?;
    let mask = if bits == 0 { 0 } else { u32::MAX << (32 - bits.min(32)) };
    Some((u32::from(ip(a)?) & mask, mask))
}

pub fn validate(c: &DhcpConfig) -> Result<(), String> {
    for sc in &c.scopes {
        let (net, mask) = cidr(&sc.network).ok_or(format!("{}: enter the network as 10.0.20.0/24", sc.name))?;
        let inside = |a: &str| ip(a).map(|x| u32::from(x) & mask == net).unwrap_or(false);
        if !inside(&sc.range_start) || !inside(&sc.range_end) {
            return Err(format!("{}: the range must be inside {}", sc.name, sc.network));
        }
        if u32::from(ip(&sc.range_start).unwrap()) > u32::from(ip(&sc.range_end).unwrap()) {
            return Err(format!("{}: the range starts after it ends", sc.name));
        }
        if !sc.gateway.is_empty() && ip(&sc.gateway).is_none() {
            return Err(format!("{}: the gateway isn't an IPv4 address", sc.name));
        }
        for d in &sc.dns {
            ip(d).ok_or(format!("{}: DNS server {d} isn't an IPv4 address", sc.name))?;
        }
        for r in &sc.reservations {
            if parse_mac(&r.mac).is_none() || !inside(&r.ip) {
                return Err(format!("{}: reservation {} / {} is invalid or outside the network", sc.name, r.mac, r.ip));
            }
        }
        if sc.lease_hours == 0 || sc.lease_hours > 24 * 90 {
            return Err(format!("{}: lease time must be 1 hour to 90 days", sc.name));
        }
    }
    if c.enabled && ip(&c.server_ip).is_none() {
        return Err("Enter this server's IP address on the network it serves".into());
    }
    Ok(())
}

pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 12 {
        return None;
    }
    let mut m = [0u8; 6];
    for i in 0..6 {
        m[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(m)
}
fn fmt_mac(m: &[u8]) -> String {
    m.iter().take(6).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

// ------------------------------------------------------------------ packets

#[derive(Debug, Clone)]
pub struct Packet {
    pub op: u8,
    pub xid: [u8; 4],
    pub flags: u16,
    pub ciaddr: Ipv4Addr,
    pub giaddr: Ipv4Addr,
    pub chaddr: [u8; 16],
    pub options: HashMap<u8, Vec<u8>>,
}

pub fn parse(b: &[u8]) -> Option<Packet> {
    if b.len() < 240 || b[236..240] != [99, 130, 83, 99] {
        return None;
    }
    let a = |i: usize| Ipv4Addr::new(b[i], b[i + 1], b[i + 2], b[i + 3]);
    let mut options = HashMap::new();
    let mut i = 240;
    while i < b.len() {
        let code = b[i];
        if code == 255 {
            break;
        }
        if code == 0 {
            i += 1;
            continue;
        }
        let len = *b.get(i + 1)? as usize;
        options.insert(code, b.get(i + 2..i + 2 + len)?.to_vec());
        i += 2 + len;
    }
    let mut chaddr = [0u8; 16];
    chaddr.copy_from_slice(&b[28..44]);
    Some(Packet {
        op: b[0],
        xid: [b[4], b[5], b[6], b[7]],
        flags: u16::from_be_bytes([b[10], b[11]]),
        ciaddr: a(12),
        giaddr: a(24),
        chaddr,
        options,
    })
}

fn reply(req: &Packet, msg_type: u8, yiaddr: Ipv4Addr, server: Ipv4Addr, opts: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let mut b = vec![0u8; 240];
    b[0] = 2;
    b[1] = 1;
    b[2] = 6;
    b[4..8].copy_from_slice(&req.xid);
    b[10..12].copy_from_slice(&req.flags.to_be_bytes());
    b[12..16].copy_from_slice(&req.ciaddr.octets());
    b[16..20].copy_from_slice(&yiaddr.octets());
    b[20..24].copy_from_slice(&server.octets());
    b[24..28].copy_from_slice(&req.giaddr.octets());
    b[28..44].copy_from_slice(&req.chaddr);
    b[236..240].copy_from_slice(&[99, 130, 83, 99]);
    b.extend([53, 1, msg_type]);
    b.extend([54, 4]);
    b.extend(server.octets());
    for (code, val) in opts {
        b.push(*code);
        b.push(val.len() as u8);
        b.extend(val);
    }
    b.push(255);
    while b.len() < 300 {
        b.push(0);
    }
    b
}

// ------------------------------------------------------------------ allocation

pub struct Engine {
    pub cfg: DhcpConfig,
    pub leases: Vec<Lease>,
}

impl Engine {
    fn scope_for(&self, giaddr: Ipv4Addr) -> Option<&Scope> {
        let pick = if giaddr.is_unspecified() { ip(&self.cfg.server_ip).unwrap_or(Ipv4Addr::UNSPECIFIED) } else { giaddr };
        self.cfg.scopes.iter().find(|s| cidr(&s.network).map(|(n, m)| u32::from(pick) & m == n).unwrap_or(false)).or_else(|| {
            if giaddr.is_unspecified() {
                self.cfg.scopes.first()
            } else {
                None
            }
        })
    }

    fn free(&self, sc: &Scope, mac: &str, want: Option<Ipv4Addr>) -> Option<Ipv4Addr> {
        let now = Utc::now();
        if let Some(r) = sc.reservations.iter().find(|r| parse_mac(&r.mac).map(|m| fmt_mac(&m)) == Some(mac.to_string())) {
            return ip(&r.ip);
        }
        let taken = |a: Ipv4Addr| {
            self.leases.iter().any(|l| l.ip == a.to_string() && l.mac != mac && l.expires > now)
                || sc.reservations.iter().any(|r| r.ip == a.to_string())
        };
        if let Some(l) = self.leases.iter().find(|l| l.mac == mac && l.scope == sc.name) {
            if let Some(a) = ip(&l.ip) {
                return Some(a);
            }
        }
        let (lo, hi) = (u32::from(ip(&sc.range_start)?), u32::from(ip(&sc.range_end)?));
        if let Some(w) = want {
            let x = u32::from(w);
            if (lo..=hi).contains(&x) && !taken(w) {
                return Some(w);
            }
        }
        (lo..=hi).map(Ipv4Addr::from).find(|a| !taken(*a))
    }

    fn options(sc: &Scope) -> Vec<(u8, Vec<u8>)> {
        let mut o = vec![];
        if let Some((_, mask)) = cidr(&sc.network) {
            o.push((1, mask.to_be_bytes().to_vec()));
        }
        if let Some(g) = ip(&sc.gateway) {
            o.push((3, g.octets().to_vec()));
        }
        let dns: Vec<u8> = sc.dns.iter().filter_map(|d| ip(d)).flat_map(|d| d.octets()).collect();
        if !dns.is_empty() {
            o.push((6, dns));
        }
        if !sc.domain.is_empty() {
            o.push((15, sc.domain.as_bytes().iter().take(255).copied().collect()));
        }
        let secs = sc.lease_hours.saturating_mul(3600);
        o.push((51, secs.to_be_bytes().to_vec()));
        o.push((58, (secs / 2).to_be_bytes().to_vec()));
        o.push((59, (secs / 8 * 7).to_be_bytes().to_vec()));
        o
    }

    /// Handle one request. Returns the reply and whether leases changed.
    pub fn handle(&mut self, req: &Packet) -> (Option<Vec<u8>>, bool) {
        let server = match ip(&self.cfg.server_ip) {
            Some(s) => s,
            None => return (None, false),
        };
        if req.op != 1 {
            return (None, false);
        }
        let msg = req.options.get(&53).and_then(|v| v.first()).copied().unwrap_or(0);
        let mac = fmt_mac(&req.chaddr);
        let hostname = req.options.get(&12).map(|h| String::from_utf8_lossy(h).to_string()).unwrap_or_default();
        let requested = req.options.get(&50).filter(|v| v.len() == 4).map(|v| Ipv4Addr::new(v[0], v[1], v[2], v[3]));
        let Some(sc) = self.scope_for(req.giaddr).cloned() else { return (None, false) };
        match msg {
            1 => {
                // DISCOVER
                let Some(a) = self.free(&sc, &mac, requested) else { return (None, false) };
                self.leases.retain(|l| !(l.mac == mac && l.offered));
                self.leases.push(Lease {
                    mac,
                    ip: a.to_string(),
                    hostname,
                    expires: Utc::now() + Duration::seconds(60),
                    scope: sc.name.clone(),
                    offered: true,
                });
                (Some(reply(req, 2, a, server, &Self::options(&sc))), true)
            }
            3 => {
                // REQUEST: selecting (server id + requested), or renewing (ciaddr).
                if let Some(sid) = req.options.get(&54) {
                    if sid.as_slice() != server.octets() {
                        self.leases.retain(|l| !(l.mac == mac && l.offered));
                        return (None, true); // the client chose another server
                    }
                }
                let want = requested.or(if req.ciaddr.is_unspecified() { None } else { Some(req.ciaddr) });
                let ok = want.and_then(|w| self.free(&sc, &mac, Some(w)).filter(|a| *a == w));
                match ok {
                    Some(a) => {
                        let hostname = if hostname.is_empty() {
                            self.leases.iter().find(|l| l.mac == mac).map(|l| l.hostname.clone()).unwrap_or_default()
                        } else {
                            hostname
                        };
                        self.leases.retain(|l| l.mac != mac);
                        let expires = Utc::now() + Duration::hours(sc.lease_hours as i64);
                        self.leases.push(Lease { mac, ip: a.to_string(), hostname, expires, scope: sc.name.clone(), offered: false });
                        (Some(reply(req, 5, a, server, &Self::options(&sc))), true)
                    }
                    None => (Some(reply(req, 6, Ipv4Addr::UNSPECIFIED, server, &[])), false),
                }
            }
            7 | 4 => {
                // RELEASE or DECLINE
                let before = self.leases.len();
                self.leases.retain(|l| l.mac != mac);
                (None, before != self.leases.len())
            }
            8 => {
                // INFORM: options only.
                let opts: Vec<(u8, Vec<u8>)> = Self::options(&sc).into_iter().filter(|(c, _)| ![51, 58, 59].contains(c)).collect();
                (Some(reply(req, 5, Ipv4Addr::UNSPECIFIED, server, &opts)), false)
            }
            _ => (None, false),
        }
    }
}

pub fn load_leases(s: &AppState) -> Vec<Lease> {
    s.db.meta_get("dhcp_leases").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}

pub async fn serve(s: AppState, cfg: DhcpConfig) -> Result<()> {
    let addr: SocketAddr = cfg.listen.parse().context("DHCP listen address")?;
    let sock = UdpSocket::bind(addr).await.with_context(|| format!("bind DHCP on UDP {addr} (port 67 needs administrator/root rights)"))?;
    sock.set_broadcast(true)?;
    tracing::info!("DHCP server listening on {addr}");
    let mut engine = Engine { cfg, leases: load_leases(&s) };
    let mut buf = vec![0u8; 1500];
    loop {
        let (n, peer) = sock.recv_from(&mut buf).await?;
        let Some(req) = parse(&buf[..n]) else { continue };
        let (resp, changed) = engine.handle(&req);
        if changed {
            let now = Utc::now();
            engine.leases.retain(|l| l.expires > now);
            if let Ok(j) = serde_json::to_string(&engine.leases) {
                let _ = s.db.meta_set("dhcp_leases", &j);
            }
        }
        if let Some(r) = resp {
            let dest: SocketAddr = if !req.giaddr.is_unspecified() {
                (req.giaddr, 67).into()
            } else if !req.ciaddr.is_unspecified() {
                (req.ciaddr, 68).into()
            } else {
                (Ipv4Addr::BROADCAST, 68).into()
            };
            let _ = sock.send_to(&r, dest).await;
            let _ = peer;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(msg: u8, mac: [u8; 6], extra: &[(u8, Vec<u8>)]) -> Packet {
        let mut chaddr = [0u8; 16];
        chaddr[..6].copy_from_slice(&mac);
        let mut options = HashMap::new();
        options.insert(53, vec![msg]);
        for (c, v) in extra {
            options.insert(*c, v.clone());
        }
        Packet { op: 1, xid: [1, 2, 3, 4], flags: 0x8000, ciaddr: Ipv4Addr::UNSPECIFIED, giaddr: Ipv4Addr::UNSPECIFIED, chaddr, options }
    }

    fn engine() -> Engine {
        Engine {
            cfg: DhcpConfig {
                enabled: true,
                listen: listen(),
                server_ip: "10.0.20.1".into(),
                scopes: vec![Scope {
                    name: "Staff".into(),
                    network: "10.0.20.0/24".into(),
                    range_start: "10.0.20.100".into(),
                    range_end: "10.0.20.101".into(),
                    gateway: "10.0.20.1".into(),
                    dns: vec!["10.0.10.5".into()],
                    domain: "corp.test".into(),
                    lease_hours: 8,
                    reservations: vec![Reservation { mac: "aa-bb-cc-00-00-09".into(), ip: "10.0.20.50".into(), name: "printer".into() }],
                }],
            },
            leases: vec![],
        }
    }

    #[test]
    fn discover_request_ack_and_exhaustion() {
        let mut e = engine();
        assert!(validate(&e.cfg).is_ok());
        let a = [0xaa, 0, 0, 0, 0, 1];
        let (offer, _) = e.handle(&packet(1, a, &[(12, b"pc-1".to_vec())]));
        let offer = parse(&offer.unwrap()).unwrap();
        assert_eq!(offer.options[&53], vec![2]);
        let yi = Ipv4Addr::new(10, 0, 20, 100);
        let (ack, _) = e.handle(&packet(3, a, &[(50, yi.octets().to_vec()), (54, vec![10, 0, 20, 1])]));
        let raw = ack.unwrap();
        assert_eq!(parse(&raw).unwrap().options[&53], vec![5]);
        assert_eq!(&raw[16..20], &yi.octets());
        assert_eq!(e.leases.len(), 1);
        assert_eq!(e.leases[0].hostname, "pc-1");

        // Second client gets the last address; a third gets nothing.
        let (o2, _) = e.handle(&packet(1, [0xaa, 0, 0, 0, 0, 2], &[]));
        assert!(o2.is_some());
        let (o3, _) = e.handle(&packet(1, [0xaa, 0, 0, 0, 0, 3], &[]));
        assert!(o3.is_none(), "range exhausted");

        // Requesting someone else's address is refused.
        let (nak, _) = e.handle(&packet(3, [0xaa, 0, 0, 0, 0, 3], &[(50, yi.octets().to_vec())]));
        assert_eq!(parse(&nak.unwrap()).unwrap().options[&53], vec![6]);

        // Reservations always get their address.
        let (o, _) = e.handle(&packet(1, [0xaa, 0xbb, 0xcc, 0, 0, 9], &[]));
        assert_eq!(&o.unwrap()[16..20], &[10, 0, 20, 50]);

        // Release frees the address.
        e.handle(&packet(7, a, &[]));
        assert!(!e.leases.iter().any(|l| l.ip == "10.0.20.100" && !l.offered));
    }

    #[test]
    fn rejects_bad_scopes() {
        let mut c = engine().cfg;
        c.scopes[0].range_end = "10.0.21.5".into();
        assert!(validate(&c).is_err());
    }
}
