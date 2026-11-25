//! A read-only LDAP server (LDAP and LDAPS) that presents the directory with
//! Active Directory-style names and attributes, so applications, NAS boxes,
//! wikis, VPNs and printers can authenticate users against OpenDirectory.
//! Writes are refused; manage the directory in the Admin Center.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use ldap3_proto::proto::*;
use ldap3_proto::LdapCodec;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_util::codec::Framed;

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LdapConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Plain LDAP (389). Leave empty to serve LDAPS only.
    #[serde(default = "ldap")]
    pub listen: String,
    /// LDAPS (636), using the Management Server's certificate.
    #[serde(default = "ldaps")]
    pub ldaps_listen: String,
    /// Let clients search without binding first.
    #[serde(default)]
    pub allow_anonymous: bool,
}
fn ldap() -> String {
    "0.0.0.0:389".into()
}
fn ldaps() -> String {
    "0.0.0.0:636".into()
}
impl Default for LdapConfig {
    fn default() -> Self {
        LdapConfig { enabled: false, listen: ldap(), ldaps_listen: ldaps(), allow_anonymous: false }
    }
}

/// `corp.example.com` → `DC=corp,DC=example,DC=com`
pub fn base_dn(domain: &str) -> String {
    domain.split('.').filter(|p| !p.is_empty()).map(|p| format!("DC={p}")).collect::<Vec<_>>().join(",")
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub dn: String,
    pub attrs: Vec<(String, Vec<String>)>,
}

impl Entry {
    fn get(&self, name: &str) -> Option<&Vec<String>> {
        self.attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    }
}

/// Every object, as AD would show it.
pub fn entries(s: &AppState) -> Vec<Entry> {
    let domain = s.db.meta_get("domain").ok().flatten().unwrap_or_default();
    let base = base_dn(&domain);
    let netbios = domain.split('.').next().unwrap_or("").to_uppercase();
    let ous = s.db.list_ous().unwrap_or_default();
    let ou_dn = |id: Option<uuid::Uuid>, fallback: &str| -> String {
        let mut parts = vec![];
        let mut cur = id;
        let mut guard = 0;
        while let Some(i) = cur {
            let Some(o) = ous.iter().find(|o| o.id == i) else { break };
            parts.push(format!("OU={}", esc(&o.name)));
            cur = o.parent_id;
            guard += 1;
            if guard > 32 {
                break;
            }
        }
        if parts.is_empty() {
            format!("{fallback},{base}")
        } else {
            format!("{},{base}", parts.join(","))
        }
    };
    let users = s.db.list_users().unwrap_or_default();
    let groups = s.db.list_groups().unwrap_or_default();
    let endpoints = s.db.list_endpoints().unwrap_or_default();
    let user_dn: HashMap<uuid::Uuid, String> =
        users.iter().map(|u| (u.id, format!("CN={},{}", esc(&u.display_name), ou_dn(u.ou_id, "CN=Users")))).collect();
    let group_dn: HashMap<uuid::Uuid, String> =
        groups.iter().map(|g| (g.id, format!("CN={},{}", esc(&g.name), ou_dn(g.ou_id, "CN=Users")))).collect();
    let pc_dn: HashMap<uuid::Uuid, String> =
        endpoints.iter().map(|e| (e.id, format!("CN={},{}", esc(&e.hostname), ou_dn(e.ou_id, "CN=Computers")))).collect();
    let v = |x: &str| vec![x.to_string()];

    let mut out = vec![Entry {
        dn: base.clone(),
        attrs: vec![
            ("objectClass".into(), vec!["top".into(), "domain".into(), "domainDNS".into()]),
            ("dc".into(), v(domain.split('.').next().unwrap_or(""))),
            ("name".into(), v(&netbios)),
        ],
    }];
    for c in ["Users", "Computers"] {
        out.push(Entry {
            dn: format!("CN={c},{base}"),
            attrs: vec![("objectClass".into(), vec!["top".into(), "container".into()]), ("cn".into(), v(c))],
        });
    }
    for o in &ous {
        out.push(Entry {
            dn: ou_dn(Some(o.id), ""),
            attrs: vec![
                ("objectClass".into(), vec!["top".into(), "organizationalUnit".into()]),
                ("ou".into(), v(&o.name)),
                ("description".into(), v(&o.description)),
                ("objectGUID".into(), v(&o.id.to_string())),
            ],
        });
    }
    for u in &users {
        let (given, sn) =
            u.display_name.split_once(' ').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((u.display_name.clone(), String::new()));
        let mut a = vec![
            (
                "objectClass".into(),
                vec!["top".into(), "person".into(), "organizationalPerson".into(), "user".into(), "inetOrgPerson".into()],
            ),
            ("cn".into(), v(&u.display_name)),
            ("sAMAccountName".into(), v(&u.username)),
            ("uid".into(), v(&u.username)),
            ("userPrincipalName".into(), v(&format!("{}@{domain}", u.username))),
            ("displayName".into(), v(&u.display_name)),
            ("givenName".into(), v(&given)),
            ("sn".into(), v(&sn)),
            ("userAccountControl".into(), v(if u.enabled { "512" } else { "514" })),
            ("objectGUID".into(), v(&u.id.to_string())),
            ("distinguishedName".into(), v(&user_dn[&u.id])),
            ("memberOf".into(), u.group_ids.iter().filter_map(|g| group_dn.get(g).cloned()).collect()),
        ];
        for (k, val) in [("mail", &u.email), ("title", &u.title), ("department", &u.department)] {
            if !val.is_empty() {
                a.push((k.into(), v(val)));
            }
        }
        out.push(Entry { dn: user_dn[&u.id].clone(), attrs: a });
    }
    for g in &groups {
        let mut members: Vec<String> = g.member_ids.iter().filter_map(|m| user_dn.get(m).cloned()).collect();
        members.extend(g.endpoint_ids.iter().filter_map(|e| pc_dn.get(e).cloned()));
        out.push(Entry {
            dn: group_dn[&g.id].clone(),
            attrs: vec![
                ("objectClass".into(), vec!["top".into(), "group".into(), "groupOfNames".into()]),
                ("cn".into(), v(&g.name)),
                ("sAMAccountName".into(), v(&g.name)),
                ("description".into(), v(&g.description)),
                ("groupType".into(), v("-2147483646")),
                ("objectGUID".into(), v(&g.id.to_string())),
                ("distinguishedName".into(), v(&group_dn[&g.id])),
                ("member".into(), members),
            ],
        });
    }
    for e in &endpoints {
        out.push(Entry {
            dn: pc_dn[&e.id].clone(),
            attrs: vec![
                (
                    "objectClass".into(),
                    vec!["top".into(), "person".into(), "organizationalPerson".into(), "user".into(), "computer".into()],
                ),
                ("cn".into(), v(&e.hostname)),
                ("sAMAccountName".into(), v(&format!("{}$", e.hostname.to_uppercase()))),
                ("dNSHostName".into(), v(&format!("{}.{domain}", e.hostname.to_lowercase()))),
                ("operatingSystem".into(), v(&e.inventory.os_name)),
                ("objectGUID".into(), v(&e.id.to_string())),
                ("distinguishedName".into(), v(&pc_dn[&e.id])),
            ],
        });
    }
    out
}

fn esc(s: &str) -> String {
    let mut o = String::new();
    for (i, c) in s.chars().enumerate() {
        if ",+\"\\<>;=".contains(c) || (i == 0 && (c == '#' || c == ' ')) {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

fn norm_dn(dn: &str) -> String {
    dn.split(',').map(|p| p.trim().to_lowercase()).collect::<Vec<_>>().join(",")
}

/// Does `filter` match this entry?
pub fn matches(f: &LdapFilter, e: &Entry) -> bool {
    let eq = |a: &str, v: &str| -> bool {
        if a.eq_ignore_ascii_case("objectCategory") {
            return e
                .get("objectClass")
                .map(|c| c.iter().any(|x| x.eq_ignore_ascii_case(v) || (v.eq_ignore_ascii_case("person") && x == "user")))
                .unwrap_or(false);
        }
        if a.eq_ignore_ascii_case("distinguishedName") || a.eq_ignore_ascii_case("member") || a.eq_ignore_ascii_case("memberOf") {
            return e.get(a).map(|vals| vals.iter().any(|x| norm_dn(x) == norm_dn(v))).unwrap_or(false);
        }
        e.get(a).map(|vals| vals.iter().any(|x| x.eq_ignore_ascii_case(v))).unwrap_or(false)
    };
    match f {
        LdapFilter::And(fs) => fs.iter().all(|x| matches(x, e)),
        LdapFilter::Or(fs) => fs.iter().any(|x| matches(x, e)),
        LdapFilter::Not(x) => !matches(x, e),
        LdapFilter::Equality(a, v) | LdapFilter::Approx(a, v) => eq(a, v),
        LdapFilter::Present(a) => a.eq_ignore_ascii_case("objectClass") || e.get(a).map(|v| !v.is_empty()).unwrap_or(false),
        LdapFilter::Substring(a, sub) => e
            .get(a)
            .map(|vals| {
                vals.iter().any(|x| {
                    let x = x.to_lowercase();
                    let mut pos = 0;
                    if let Some(i) = &sub.initial {
                        if !x.starts_with(&i.to_lowercase()) {
                            return false;
                        }
                        pos = i.len();
                    }
                    for part in &sub.any {
                        match x[pos.min(x.len())..].find(&part.to_lowercase()) {
                            Some(p) => pos += p + part.len(),
                            None => return false,
                        }
                    }
                    sub.final_.as_ref().map(|fin| x[pos.min(x.len())..].ends_with(&fin.to_lowercase())).unwrap_or(true)
                })
            })
            .unwrap_or(false),
        LdapFilter::GreaterOrEqual(a, v) => e.get(a).map(|vals| vals.iter().any(|x| x.as_str() >= v.as_str())).unwrap_or(false),
        LdapFilter::LessOrEqual(a, v) => e.get(a).map(|vals| vals.iter().any(|x| x.as_str() <= v.as_str())).unwrap_or(false),
        _ => false,
    }
}

fn in_scope(dn: &str, base: &str, scope: &LdapSearchScope) -> bool {
    let (d, b) = (norm_dn(dn), norm_dn(base));
    match scope {
        LdapSearchScope::Base => d == b,
        LdapSearchScope::OneLevel => d
            .strip_suffix(&b)
            .and_then(|r| r.strip_suffix(','))
            .map(|r| !r.contains(',') || r.matches(',').count() == r.matches("\\,").count())
            .unwrap_or(false),
        _ => b.is_empty() || d == b || d.ends_with(&format!(",{b}")),
    }
}

fn result(code: LdapResultCode, msg: &str) -> LdapResult {
    LdapResult { code, matcheddn: String::new(), message: msg.into(), referral: vec![] }
}

/// The username a bind DN refers to, in any of the forms clients use.
fn bind_user(s: &AppState, dn: &str) -> Option<od_core::User> {
    let d = dn.trim();
    let lower = d.to_lowercase();
    let candidate = if let Some(rest) = lower.strip_prefix("uid=").or_else(|| lower.strip_prefix("samaccountname=")) {
        rest.split(',').next().unwrap_or("").to_string()
    } else if lower.starts_with("cn=") {
        let norm = norm_dn(d);
        return entries(s)
            .into_iter()
            .find(|e| norm_dn(&e.dn) == norm)
            .and_then(|e| e.get("sAMAccountName").and_then(|v| v.first().cloned()))
            .and_then(|n| s.db.get_user_by_username(&n).ok().flatten());
    } else {
        lower.rsplit('\\').next().unwrap_or("").split('@').next().unwrap_or("").to_string()
    };
    s.db.get_user_by_username(&candidate).ok().flatten()
}

async fn session<T: AsyncRead + AsyncWrite + Unpin>(s: AppState, cfg: LdapConfig, io: T, peer: SocketAddr) {
    let mut framed = Framed::new(io, LdapCodec::default());
    let mut bound: Option<String> = None;
    while let Some(Ok(msg)) = framed.next().await {
        let id = msg.msgid;
        let mut replies: Vec<LdapOp> = vec![];
        match msg.op {
            LdapOp::BindRequest(b) => {
                let r = match (&b.cred, b.dn.is_empty()) {
                    (LdapBindCred::Simple(pw), true) if pw.is_empty() => {
                        bound = None;
                        result(LdapResultCode::Success, "")
                    }
                    (LdapBindCred::Simple(pw), _) => {
                        let user = bind_user(&s, &b.dn);
                        let name = user.as_ref().map(|u| u.username.clone()).unwrap_or_else(|| b.dn.chars().take(64).collect());
                        if crate::api::directory::locked_out(&s, &name).is_some() {
                            result(LdapResultCode::InvalidCredentials, "account locked out")
                        } else {
                            let ok = user
                                .as_ref()
                                .filter(|u| u.enabled)
                                .and_then(|u| s.db.user_secrets(u.id).ok())
                                .and_then(|sec| sec.password_hash)
                                .map(|h| !pw.is_empty() && crate::crypto::verify_password(pw, &h))
                                .unwrap_or(false);
                            if ok {
                                s.login_failures.lock().unwrap_or_else(|e| e.into_inner()).remove(&name);
                                bound = Some(name);
                                result(LdapResultCode::Success, "")
                            } else {
                                crate::api::directory::record_failed_login(&s, &name, &format!("LDAP bind from {}", peer.ip()));
                                bound = None;
                                result(LdapResultCode::InvalidCredentials, "")
                            }
                        }
                    }
                    _ => result(LdapResultCode::AuthMethodNotSupported, "Use a simple bind (over LDAPS)"),
                };
                replies.push(LdapOp::BindResponse(LdapBindResponse { res: r, saslcreds: None }));
            }
            LdapOp::SearchRequest(q) => {
                let domain = s.db.meta_get("domain").ok().flatten().unwrap_or_default();
                if q.base.is_empty() && q.scope == LdapSearchScope::Base {
                    // Root DSE: what this server is and where the directory starts.
                    let base = base_dn(&domain);
                    let attrs = [
                        ("namingContexts", base.clone()),
                        ("defaultNamingContext", base),
                        ("supportedLDAPVersion", "3".into()),
                        ("vendorName", "OpenDirectory".into()),
                        ("vendorVersion", crate::VERSION.into()),
                        ("supportedExtension", "1.3.6.1.4.1.4203.1.11.3".into()),
                    ];
                    replies.push(LdapOp::SearchResultEntry(LdapSearchResultEntry {
                        dn: String::new(),
                        attributes: attrs
                            .into_iter()
                            .map(|(k, v)| LdapPartialAttribute { atype: k.into(), vals: vec![v.into_bytes()] })
                            .collect(),
                    }));
                    replies.push(LdapOp::SearchResultDone(result(LdapResultCode::Success, "")));
                } else if bound.is_none() && !cfg.allow_anonymous {
                    replies.push(LdapOp::SearchResultDone(result(LdapResultCode::InsufficentAccessRights, "Bind first")));
                } else {
                    let want_all = q.attrs.is_empty() || q.attrs.iter().any(|a| a == "*");
                    let mut n = 0;
                    for e in entries(&s).into_iter().filter(|e| in_scope(&e.dn, &q.base, &q.scope) && matches(&q.filter, e)) {
                        n += 1;
                        if q.sizelimit > 0 && n > q.sizelimit {
                            break;
                        }
                        let attributes = e
                            .attrs
                            .iter()
                            .filter(|(k, v)| !v.is_empty() && (want_all || q.attrs.iter().any(|a| a.eq_ignore_ascii_case(k))))
                            .map(|(k, v)| LdapPartialAttribute {
                                atype: k.clone(),
                                vals: if q.typesonly { vec![] } else { v.iter().map(|x| x.clone().into_bytes()).collect() },
                            })
                            .collect();
                        replies.push(LdapOp::SearchResultEntry(LdapSearchResultEntry { dn: e.dn.clone(), attributes }));
                    }
                    replies.push(LdapOp::SearchResultDone(result(LdapResultCode::Success, "")));
                }
            }
            LdapOp::ExtendedRequest(x) if x.name == "1.3.6.1.4.1.4203.1.11.3" => {
                let who = bound.as_ref().map(|u| format!("u:{u}")).unwrap_or_default();
                replies.push(LdapOp::ExtendedResponse(LdapExtendedResponse {
                    res: result(LdapResultCode::Success, ""),
                    name: None,
                    value: Some(who.into_bytes()),
                }));
            }
            LdapOp::ExtendedRequest(_) => replies.push(LdapOp::ExtendedResponse(LdapExtendedResponse {
                res: result(LdapResultCode::ProtocolError, "Not supported; use LDAPS"),
                name: None,
                value: None,
            })),
            LdapOp::UnbindRequest => break,
            LdapOp::ModifyRequest(_) => {
                replies.push(LdapOp::ModifyResponse(result(LdapResultCode::UnwillingToPerform, "This directory is read-only over LDAP")))
            }
            LdapOp::AddRequest(_) => {
                replies.push(LdapOp::AddResponse(result(LdapResultCode::UnwillingToPerform, "This directory is read-only over LDAP")))
            }
            LdapOp::DelRequest(_) => {
                replies.push(LdapOp::DelResponse(result(LdapResultCode::UnwillingToPerform, "This directory is read-only over LDAP")))
            }
            LdapOp::ModifyDNRequest(_) => {
                replies.push(LdapOp::ModifyDNResponse(result(LdapResultCode::UnwillingToPerform, "This directory is read-only over LDAP")))
            }
            LdapOp::CompareRequest(_) => {
                replies.push(LdapOp::CompareResult(result(LdapResultCode::UnwillingToPerform, "Compare isn't supported")))
            }
            _ => {}
        }
        for op in replies {
            if framed.send(LdapMsg { msgid: id, op, ctrl: vec![] }).await.is_err() {
                return;
            }
        }
    }
}

fn tls_acceptor(data_dir: &std::path::Path) -> Result<tokio_rustls::TlsAcceptor> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(data_dir.join("tls-cert.pem"))?.collect::<Result<_, _>>()?;
    let key = PrivateKeyDer::from_pem_file(data_dir.join("tls-key.pem"))?;
    let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(cfg)))
}

pub async fn serve(s: AppState, cfg: LdapConfig) -> Result<()> {
    let mut tasks = vec![];
    if !cfg.listen.trim().is_empty() {
        let addr: SocketAddr = cfg.listen.parse().context("LDAP listen address")?;
        let l = TcpListener::bind(addr)
            .await
            .with_context(|| format!("bind LDAP on {addr} (ports below 1024 need administrator/root rights)"))?;
        tracing::info!("LDAP server listening on {addr}");
        let (s, cfg) = (s.clone(), cfg.clone());
        tasks.push(tokio::spawn(async move {
            while let Ok((c, peer)) = l.accept().await {
                tokio::spawn(session(s.clone(), cfg.clone(), c, peer));
            }
        }));
    }
    if !cfg.ldaps_listen.trim().is_empty() {
        let addr: SocketAddr = cfg.ldaps_listen.parse().context("LDAPS listen address")?;
        let acceptor = tls_acceptor(std::path::Path::new(&s.info.data_dir))
            .context("LDAPS needs the server's TLS certificate (it isn't available in plain-HTTP mode)")?;
        let l = TcpListener::bind(addr).await.with_context(|| format!("bind LDAPS on {addr}"))?;
        tracing::info!("LDAPS server listening on {addr}");
        let (s, cfg) = (s.clone(), cfg.clone());
        tasks.push(tokio::spawn(async move {
            while let Ok((c, peer)) = l.accept().await {
                let (s, cfg, acceptor) = (s.clone(), cfg.clone(), acceptor.clone());
                tokio::spawn(async move {
                    if let Ok(tls) = acceptor.accept(c).await {
                        session(s, cfg, tls, peer).await;
                    }
                });
            }
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_and_scope_helpers() {
        assert_eq!(base_dn("corp.example.com"), "DC=corp,DC=example,DC=com");
        assert!(in_scope("CN=Dan,OU=Staff,DC=corp,DC=test", "dc=corp,dc=test", &LdapSearchScope::Subtree));
        assert!(!in_scope("CN=Dan,OU=Staff,DC=corp,DC=test", "dc=corp,dc=test", &LdapSearchScope::OneLevel));
        assert!(in_scope("OU=Staff,DC=corp,DC=test", "DC=corp,DC=test", &LdapSearchScope::OneLevel));
        assert_eq!(esc("Smith, John"), "Smith\\, John");
    }

    #[test]
    fn filters() {
        let e = Entry {
            dn: "CN=Dan,DC=t".into(),
            attrs: vec![
                ("objectClass".into(), vec!["user".into(), "person".into()]),
                ("sAMAccountName".into(), vec!["dan".into()]),
                ("mail".into(), vec!["dan@corp.test".into()]),
            ],
        };
        let f =
            ldap3_proto::parse_ldap_filter_str("(&(objectClass=user)(|(sAMAccountName=DAN)(mail=x))(mail=*@corp.test)(!(cn=x)))").unwrap();
        assert!(matches(&f, &e));
        let f = ldap3_proto::parse_ldap_filter_str("(&(objectCategory=person)(uid=bob))").unwrap();
        assert!(!matches(&f, &e));
    }
}
