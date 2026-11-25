//! A RADIUS server (like NPS) that authenticates directory users for VPNs,
//! network devices and captive portals. Supports PAP and MS-CHAPv2, the
//! Message-Authenticator attribute, and per-client required groups.
//! EAP (WPA2/3-Enterprise Wi-Fi with PEAP or EAP-TLS) is not supported yet.

use std::net::{IpAddr, SocketAddr};

use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use md5::{Digest as _, Md5};
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RadiusClient {
    pub name: String,
    /// IP address or CIDR range of the NAS (VPN gateway, switch, access point).
    pub address: String,
    pub secret: String,
    /// Only members of this security group are accepted (empty: any enabled user).
    #[serde(default)]
    pub required_group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RadiusConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "listen")]
    pub listen: String,
    #[serde(default)]
    pub clients: Vec<RadiusClient>,
}
fn listen() -> String {
    "0.0.0.0:1812".into()
}

const ACCESS_REQUEST: u8 = 1;
const ACCESS_ACCEPT: u8 = 2;
const ACCESS_REJECT: u8 = 3;

pub struct Request {
    pub code: u8,
    pub id: u8,
    pub authenticator: [u8; 16],
    pub attrs: Vec<(u8, Vec<u8>)>,
    raw: Vec<u8>,
}

pub fn parse(b: &[u8]) -> Option<Request> {
    if b.len() < 20 {
        return None;
    }
    let len = u16::from_be_bytes([b[2], b[3]]) as usize;
    if len < 20 || len > b.len() {
        return None;
    }
    let mut attrs = vec![];
    let mut i = 20;
    while i + 2 <= len {
        let (t, l) = (b[i], b[i + 1] as usize);
        if l < 2 || i + l > len {
            return None;
        }
        attrs.push((t, b[i + 2..i + l].to_vec()));
        i += l;
    }
    let mut authenticator = [0u8; 16];
    authenticator.copy_from_slice(&b[4..20]);
    Some(Request { code: b[0], id: b[1], authenticator, attrs, raw: b[..len].to_vec() })
}

impl Request {
    fn attr(&self, t: u8) -> Option<&[u8]> {
        self.attrs.iter().find(|(a, _)| *a == t).map(|(_, v)| v.as_slice())
    }
    /// Vendor-specific attribute (26) for vendor and type.
    fn vsa(&self, vendor: u32, vtype: u8) -> Option<Vec<u8>> {
        self.attrs.iter().filter(|(t, _)| *t == 26).find_map(|(_, v)| {
            if v.len() < 6 || u32::from_be_bytes([v[0], v[1], v[2], v[3]]) != vendor || v[4] != vtype {
                return None;
            }
            Some(v[6..(v[5] as usize + 4).min(v.len())].to_vec())
        })
    }
}

/// Decrypt PAP User-Password (RFC 2865 §5.2).
pub fn pap_decrypt(secret: &[u8], authenticator: &[u8; 16], enc: &[u8]) -> Option<String> {
    if enc.is_empty() || enc.len() % 16 != 0 || enc.len() > 128 {
        return None;
    }
    let mut out = Vec::with_capacity(enc.len());
    let mut prev: Vec<u8> = authenticator.to_vec();
    for chunk in enc.chunks(16) {
        let b = Md5::new().chain_update(secret).chain_update(&prev).finalize();
        out.extend(chunk.iter().zip(b.iter()).map(|(c, k)| c ^ k));
        prev = chunk.to_vec();
    }
    while out.last() == Some(&0) {
        out.pop();
    }
    String::from_utf8(out).ok()
}

// ------------------------------------------------------------------ MS-CHAPv2 (RFC 2759)

pub fn nt_hash(password: &str) -> [u8; 16] {
    use md4::Md4;
    let utf16: Vec<u8> = password.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    Md4::digest(&utf16).into()
}

fn challenge_hash(peer: &[u8], auth: &[u8], user: &str) -> [u8; 8] {
    use sha1::{Digest, Sha1};
    let h = Sha1::new().chain_update(peer).chain_update(auth).chain_update(user.as_bytes()).finalize();
    h[..8].try_into().expect("8 bytes")
}

fn des_key(k7: &[u8]) -> [u8; 8] {
    let mut k = [0u8; 8];
    k[0] = k7[0];
    k[1] = (k7[0] << 7) | (k7[1] >> 1);
    k[2] = (k7[1] << 6) | (k7[2] >> 2);
    k[3] = (k7[2] << 5) | (k7[3] >> 3);
    k[4] = (k7[3] << 4) | (k7[4] >> 4);
    k[5] = (k7[4] << 3) | (k7[5] >> 5);
    k[6] = (k7[5] << 2) | (k7[6] >> 6);
    k[7] = k7[6] << 1;
    k
}

fn challenge_response(challenge: &[u8; 8], pw_hash: &[u8; 16]) -> [u8; 24] {
    use des::cipher::{BlockEncrypt, KeyInit};
    let mut z = [0u8; 21];
    z[..16].copy_from_slice(pw_hash);
    let mut out = [0u8; 24];
    for i in 0..3 {
        let c = des::Des::new_from_slice(&des_key(&z[i * 7..i * 7 + 7])).expect("8-byte key");
        let mut block = des::cipher::generic_array::GenericArray::clone_from_slice(challenge);
        c.encrypt_block(&mut block);
        out[i * 8..i * 8 + 8].copy_from_slice(&block);
    }
    out
}

pub fn mschapv2_nt_response(auth_challenge: &[u8], peer_challenge: &[u8], user: &str, password: &str) -> [u8; 24] {
    challenge_response(&challenge_hash(peer_challenge, auth_challenge, user), &nt_hash(password))
}

pub fn mschapv2_authenticator_response(
    password: &str,
    nt_response: &[u8],
    peer_challenge: &[u8],
    auth_challenge: &[u8],
    user: &str,
) -> String {
    use md4::Md4;
    use sha1::{Digest, Sha1};
    const MAGIC1: &[u8] = b"Magic server to client signing constant";
    const MAGIC2: &[u8] = b"Pad to make it do more than one iteration";
    let hh = Md4::digest(nt_hash(password));
    let d = Sha1::new().chain_update(hh).chain_update(nt_response).chain_update(MAGIC1).finalize();
    let d = Sha1::new().chain_update(d).chain_update(challenge_hash(peer_challenge, auth_challenge, user)).chain_update(MAGIC2).finalize();
    format!("S={}", hex::encode_upper(d))
}

// ------------------------------------------------------------------ responses

fn build(code: u8, req: &Request, secret: &[u8], mut attrs: Vec<(u8, Vec<u8>)>) -> Vec<u8> {
    let with_ma = req.attr(80).is_some();
    if with_ma {
        attrs.push((80, vec![0u8; 16]));
    }
    let mut out = vec![code, req.id, 0, 0];
    out.extend(req.authenticator);
    for (t, v) in &attrs {
        out.push(*t);
        out.push((v.len() + 2) as u8);
        out.extend(v);
    }
    let len = out.len() as u16;
    out[2..4].copy_from_slice(&len.to_be_bytes());
    if with_ma {
        // HMAC-MD5 over the packet with the request authenticator in place (RFC 3579 §3.2).
        let pos = out.len() - 16;
        let mut m = <Hmac<Md5> as Mac>::new_from_slice(secret).expect("any key");
        m.update(&out);
        out[pos..].copy_from_slice(&m.finalize().into_bytes());
    }
    let ra = Md5::new().chain_update(&out).chain_update(secret).finalize();
    out[4..20].copy_from_slice(&ra);
    out
}

fn message_authenticator_ok(req: &Request, secret: &[u8]) -> bool {
    let Some(ma) = req.attr(80) else { return true };
    let mut raw = req.raw.clone();
    // Zero the attribute's value in place.
    let mut i = 20;
    while i + 2 <= raw.len() {
        let (t, l) = (raw[i], raw[i + 1] as usize);
        if t == 80 && l == 18 {
            raw[i + 2..i + 18].fill(0);
        }
        i += l.max(2);
    }
    let mut m = <Hmac<Md5> as Mac>::new_from_slice(secret).expect("any key");
    m.update(&raw);
    crate::crypto::ct_eq(&hex::encode(m.finalize().into_bytes()), &hex::encode(ma))
}

fn client_for<'a>(cfg: &'a RadiusConfig, ip: IpAddr) -> Option<&'a RadiusClient> {
    cfg.clients.iter().find(|c| {
        if let Ok(a) = c.address.parse::<IpAddr>() {
            return a == ip;
        }
        let (net, bits) = match c.address.split_once('/') {
            Some((n, b)) => (n.parse::<IpAddr>().ok(), b.parse::<u32>().ok()),
            None => (None, None),
        };
        match (net, bits, ip) {
            (Some(IpAddr::V4(n)), Some(b), IpAddr::V4(i)) if b <= 32 => {
                let m = if b == 0 { 0 } else { u32::MAX << (32 - b) };
                u32::from(n) & m == u32::from(i) & m
            }
            _ => false,
        }
    })
}

/// Decide on one request. Returns the reply packet.
pub fn handle(s: &AppState, cfg: &RadiusConfig, peer: IpAddr, packet: &[u8]) -> Option<Vec<u8>> {
    let req = parse(packet)?;
    if req.code != ACCESS_REQUEST {
        return None;
    }
    let client = client_for(cfg, peer)?; // unknown NAS: silently drop, as RFC 2865 says
    let secret = client.secret.as_bytes();
    if !message_authenticator_ok(&req, secret) {
        return None;
    }
    let reject = |msg: &str| Some(build(ACCESS_REJECT, &req, secret, vec![(18, msg.as_bytes().iter().take(200).copied().collect())]));
    if req.attr(79).is_some() {
        return reject("EAP (WPA2/3-Enterprise) isn't supported by OpenDirectory yet");
    }
    let raw_user = String::from_utf8_lossy(req.attr(1).unwrap_or_default()).to_string();
    let name = raw_user.trim().to_lowercase();
    let name = name.rsplit('\\').next().unwrap_or(&name).split('@').next().unwrap_or("").to_string();
    if crate::api::directory::locked_out(s, &name).is_some() {
        return reject("Account locked out");
    }
    let user = s.db.get_user_by_username(&name).ok().flatten();
    let fail = |why: &str| {
        crate::api::directory::record_failed_login(s, &name, &format!("RADIUS ({}): {why}", client.name));
        reject("Access denied")
    };
    let Some(user) = user else { return fail("unknown account") };
    if !user.enabled {
        return fail("account disabled");
    }
    if !client.required_group.is_empty() {
        let in_group =
            s.db.list_groups()
                .unwrap_or_default()
                .iter()
                .any(|g| g.name.eq_ignore_ascii_case(&client.required_group) && g.member_ids.contains(&user.id));
        if !in_group {
            return fail(&format!("not in {}", client.required_group));
        }
    }
    let secrets = s.db.user_secrets(user.id).ok()?;
    let mut reply_attrs = vec![];
    if let Some(enc) = req.attr(2) {
        let pw = pap_decrypt(secret, &req.authenticator, enc).unwrap_or_default();
        let ok = secrets.password_hash.as_deref().map(|h| crate::crypto::verify_password(&pw, h)).unwrap_or(false);
        if !ok {
            return fail("wrong password (PAP)");
        }
    } else if let (Some(chal), Some(resp)) = (req.vsa(311, 11), req.vsa(311, 25)) {
        if resp.len() != 50 || chal.len() != 16 {
            return fail("malformed MS-CHAPv2");
        }
        let Some(pw) = secrets.password_sealed.and_then(|b| s.sealer.open(&b).ok()).and_then(|b| String::from_utf8(b).ok()) else {
            return fail("no password set");
        };
        let (ident, peer, nt) = (resp[0], &resp[2..18], &resp[26..50]);
        // MS-CHAPv2 hashes the user name exactly as sent, without domain.
        let user_for_hash = raw_user.rsplit('\\').next().unwrap_or(&raw_user).to_string();
        if mschapv2_nt_response(&chal, peer, &user_for_hash, &pw) != nt {
            return fail("wrong password (MS-CHAPv2)");
        }
        let auth = mschapv2_authenticator_response(&pw, nt, peer, &chal, &user_for_hash);
        let mut v = 311u32.to_be_bytes().to_vec();
        v.push(26);
        v.push((auth.len() + 3) as u8);
        v.push(ident);
        v.extend(auth.as_bytes());
        reply_attrs.push((26, v));
    } else {
        return reject("Use PAP or MS-CHAPv2");
    }
    s.login_failures.lock().unwrap_or_else(|e| e.into_inner()).remove(&name);
    s.db.audit(&user.username, "radius.accept", &client.name, "");
    reply_attrs.push((18, format!("Welcome, {}", user.display_name).into_bytes()));
    Some(build(ACCESS_ACCEPT, &req, secret, reply_attrs))
}

pub async fn serve(s: AppState, cfg: RadiusConfig) -> Result<()> {
    let addr: SocketAddr = cfg.listen.parse().context("RADIUS listen address")?;
    let sock = UdpSocket::bind(addr).await.with_context(|| format!("bind RADIUS on UDP {addr}"))?;
    tracing::info!("RADIUS server listening on {addr}");
    let mut buf = vec![0u8; 4096];
    loop {
        let (n, peer) = sock.recv_from(&mut buf).await?;
        if let Some(r) = handle(&s, &cfg, peer.ip(), &buf[..n]) {
            let _ = sock.send_to(&r, peer).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 2759 section 9.2 test vectors.
    #[test]
    fn mschapv2_matches_rfc_2759() {
        let auth = hex::decode("5B5D7C7D7B3F2F3E3C2C602132262628").unwrap();
        let peer = hex::decode("21402324255E262A28295F2B3A337C7E").unwrap();
        assert_eq!(hex::encode_upper(nt_hash("clientPass")), "44EBBA8D5312B8D611474411F56989AE");
        assert_eq!(hex::encode_upper(challenge_hash(&peer, &auth, "User")), "D02E4386BCE91226");
        let nt = mschapv2_nt_response(&auth, &peer, "User", "clientPass");
        assert_eq!(hex::encode_upper(nt), "82309ECD8D708B5EA08FAA3981CD83544233114A3D85D6DF");
        assert_eq!(mschapv2_authenticator_response("clientPass", &nt, &peer, &auth, "User"), "S=407A5589115FD0D6209F510FE9C04566932CDA56");
    }

    #[test]
    fn pap_roundtrip() {
        let secret = b"s3cret";
        let ra = [7u8; 16];
        let pw = b"hello-world-password";
        let mut padded = pw.to_vec();
        padded.resize(32, 0);
        let mut enc = vec![];
        let mut prev = ra.to_vec();
        for chunk in padded.chunks(16) {
            let b = Md5::new().chain_update(secret).chain_update(&prev).finalize();
            let c: Vec<u8> = chunk.iter().zip(b.iter()).map(|(p, k)| p ^ k).collect();
            prev = c.clone();
            enc.extend(c);
        }
        assert_eq!(pap_decrypt(secret, &ra, &enc).unwrap(), "hello-world-password");
    }
}
