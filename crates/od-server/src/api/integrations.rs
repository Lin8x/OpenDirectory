//! Integrations: connect OpenDirectory to existing Windows services and cloud
//! apps. Active Directory (LDAP) and Microsoft Entra ID can import users and
//! groups; webhooks, Slack and Teams receive audit events; SMB, DNS, DHCP and
//! WSUS get a guided connection test.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use od_core::{AuditEntry, Group, OrgUnit, Role, User};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::validate_username;
use crate::auth::Admin;
use crate::destinations::KEEP;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Connector {
    /// An existing Active Directory (or Samba AD) domain controller, over LDAP/LDAPS.
    ActiveDirectory {
        /// ldaps://dc01.corp.example.com:636 or ldap://dc01:389
        url: String,
        /// e.g. CN=svc-opendirectory,OU=Service Accounts,DC=corp,DC=example,DC=com or svc@corp.example.com
        bind_dn: String,
        #[serde(default)]
        password: String,
        /// e.g. DC=corp,DC=example,DC=com
        base_dn: String,
        #[serde(default)]
        starttls: bool,
        /// Accept a self-signed DC certificate (lab use).
        #[serde(default)]
        skip_tls_verify: bool,
    },
    /// Microsoft Entra ID (Azure AD / Microsoft 365) via Microsoft Graph.
    EntraId {
        tenant_id: String,
        client_id: String,
        #[serde(default)]
        client_secret: String,
    },
    /// POST every matching audit event as JSON.
    Webhook {
        url: String,
        /// Signs each body with HMAC-SHA256 in the X-OpenDirectory-Signature header.
        #[serde(default)]
        secret: String,
        /// Action prefixes to send, e.g. ["backup.", "endpoint.enroll"]. Empty sends everything.
        #[serde(default)]
        events: Vec<String>,
    },
    /// Slack incoming webhook.
    Slack {
        webhook_url: String,
        #[serde(default)]
        events: Vec<String>,
    },
    /// Microsoft Teams incoming webhook / Workflows URL.
    Teams {
        webhook_url: String,
        #[serde(default)]
        events: Vec<String>,
    },
    /// A Windows service on the network, with a reachability test and a setup guide.
    WindowsService {
        /// smb, dns, dhcp, wsus, rds, print, ca
        service: String,
        host: String,
        #[serde(default)]
        port: Option<u16>,
    },
}

impl Connector {
    fn kind(&self) -> &'static str {
        match self {
            Connector::ActiveDirectory { .. } => "active_directory",
            Connector::EntraId { .. } => "entra_id",
            Connector::Webhook { .. } => "webhook",
            Connector::Slack { .. } => "slack",
            Connector::Teams { .. } => "teams",
            Connector::WindowsService { .. } => "windows_service",
        }
    }
    fn secrets_mut(&mut self) -> Vec<&mut String> {
        match self {
            Connector::ActiveDirectory { password, .. } => vec![password],
            Connector::EntraId { client_secret, .. } => vec![client_secret],
            Connector::Webhook { secret, .. } => vec![secret],
            Connector::Slack { webhook_url, .. } | Connector::Teams { webhook_url, .. } => vec![webhook_url],
            Connector::WindowsService { .. } => vec![],
        }
    }
    fn redacted(&self) -> Connector {
        let mut c = self.clone();
        for s in c.secrets_mut() {
            if !s.is_empty() {
                *s = KEEP.into();
            }
        }
        c
    }
    fn merge_secrets(&mut self, old: &Connector) {
        let mut old = old.clone();
        let same = std::mem::discriminant(self) == std::mem::discriminant(&old);
        let olds: Vec<String> = if same { old.secrets_mut().into_iter().map(std::mem::take).collect() } else { vec![] };
        for (i, s) in self.secrets_mut().into_iter().enumerate() {
            if s == KEEP {
                *s = olds.get(i).cloned().unwrap_or_default();
            }
        }
    }
    fn can_import(&self) -> bool {
        matches!(self, Connector::ActiveDirectory { .. } | Connector::EntraId { .. })
    }
    fn wants(&self, action: &str) -> bool {
        let events = match self {
            Connector::Webhook { events, .. } | Connector::Slack { events, .. } | Connector::Teams { events, .. } => events,
            _ => return false,
        };
        events.is_empty() || events.iter().any(|e| action.starts_with(e.as_str()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stored {
    pub id: Uuid,
    pub name: String,
    pub connector: Connector,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_sync_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_ok: Option<bool>,
    #[serde(default)]
    pub last_message: String,
}

#[derive(Serialize)]
pub struct View {
    id: Uuid,
    name: String,
    kind: &'static str,
    connector: Connector,
    enabled: bool,
    can_import: bool,
    last_sync_at: Option<DateTime<Utc>>,
    last_ok: Option<bool>,
    last_message: String,
}

impl From<&Stored> for View {
    fn from(s: &Stored) -> Self {
        View {
            id: s.id,
            name: s.name.clone(),
            kind: s.connector.kind(),
            connector: s.connector.redacted(),
            enabled: s.enabled,
            can_import: s.connector.can_import(),
            last_sync_at: s.last_sync_at,
            last_ok: s.last_ok,
            last_message: s.last_message.clone(),
        }
    }
}

fn load_all(s: &AppState) -> anyhow::Result<Vec<Stored>> {
    let mut v = vec![];
    for (_, sealed) in s.db.list_integrations()? {
        if let Ok(x) = s.sealer.open(&sealed).and_then(|b| Ok(serde_json::from_slice::<Stored>(&b)?)) {
            v.push(x);
        }
    }
    v.sort_by_key(|x| x.name.to_lowercase());
    Ok(v)
}
fn load(s: &AppState, id: Uuid) -> ApiResult<Stored> {
    load_all(s)?.into_iter().find(|x| x.id == id).ok_or_else(|| ApiError::not_found("Integration"))
}
fn save(s: &AppState, x: &Stored) -> anyhow::Result<()> {
    s.db.upsert_integration(x.id, x.connector.kind(), &s.sealer.seal(&serde_json::to_vec(x)?)?)
}

pub async fn list(State(s): State<AppState>, _a: Admin) -> ApiResult<Json<Vec<View>>> {
    Ok(Json(load_all(&s)?.iter().map(View::from).collect()))
}

#[derive(Deserialize)]
pub struct Input {
    name: String,
    connector: Connector,
    #[serde(default = "yes")]
    enabled: bool,
}
fn yes() -> bool {
    true
}

fn validate(c: &Connector) -> ApiResult<()> {
    let bad = |m: &str| Err(ApiError::bad_request(m.to_string()));
    match c {
        Connector::ActiveDirectory { url, bind_dn, base_dn, .. } => {
            if !url.starts_with("ldap://") && !url.starts_with("ldaps://") {
                return bad("The URL must start with ldaps:// or ldap://");
            }
            if bind_dn.trim().is_empty() || base_dn.trim().is_empty() {
                return bad("Enter the bind account and the base DN");
            }
        }
        Connector::EntraId { tenant_id, client_id, .. } => {
            if tenant_id.trim().is_empty() || client_id.trim().is_empty() {
                return bad("Enter the tenant ID and client ID");
            }
        }
        Connector::Webhook { url, .. } => {
            if !url.starts_with("https://") && !url.starts_with("http://") {
                return bad("The URL must start with https://");
            }
        }
        Connector::Slack { webhook_url, .. } | Connector::Teams { webhook_url, .. } => {
            if !webhook_url.starts_with("https://") && webhook_url != KEEP {
                return bad("Paste the incoming webhook URL (it starts with https://)");
            }
        }
        Connector::WindowsService { host, service, .. } => {
            if host.trim().is_empty() {
                return bad("Enter the server's host name or IP address");
            }
            if default_port(service).is_none() {
                return bad("Unknown service");
            }
        }
    }
    Ok(())
}

pub async fn create(State(s): State<AppState>, admin: Admin, Json(inp): Json<Input>) -> ApiResult<(StatusCode, Json<View>)> {
    validate(&inp.connector)?;
    let x = Stored {
        id: Uuid::new_v4(),
        name: super::required("Name", &inp.name)?,
        connector: inp.connector,
        enabled: inp.enabled,
        created_at: Utc::now(),
        last_sync_at: None,
        last_ok: None,
        last_message: String::new(),
    };
    save(&s, &x)?;
    s.db.audit(admin.name(), "integration.create", &x.name, x.connector.kind());
    Ok((StatusCode::CREATED, Json((&x).into())))
}

pub async fn update(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>, Json(inp): Json<Input>) -> ApiResult<Json<View>> {
    let mut x = load(&s, id)?;
    let mut c = inp.connector;
    c.merge_secrets(&x.connector);
    validate(&c)?;
    x.connector = c;
    x.name = super::required("Name", &inp.name)?;
    x.enabled = inp.enabled;
    save(&s, &x)?;
    s.db.audit(admin.name(), "integration.update", &x.name, "");
    Ok(Json((&x).into()))
}

pub async fn remove(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let x = load(&s, id)?;
    s.db.delete_integration(id)?;
    s.db.audit(admin.name(), "integration.delete", &x.name, "");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct TestResult {
    ok: bool,
    message: String,
}

pub async fn test(State(s): State<AppState>, _a: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<TestResult>> {
    let x = load(&s, id)?;
    Ok(Json(match run_test(&x.connector).await {
        Ok(m) => TestResult { ok: true, message: m },
        Err(e) => TestResult { ok: false, message: format!("{e:#}") },
    }))
}

fn default_port(service: &str) -> Option<u16> {
    Some(match service {
        "smb" => 445,
        "dns" => 53,
        "dhcp" => 67,
        "wsus" => 8530,
        "rds" => 3389,
        "print" => 445,
        "ca" => 135,
        "kerberos" => 88,
        "ldap" => 389,
        _ => return None,
    })
}

async fn run_test(c: &Connector) -> anyhow::Result<String> {
    match c {
        Connector::ActiveDirectory { .. } => {
            let (users, groups) = ad::fetch(c, Some(5)).await?;
            Ok(format!("Signed in to the domain controller. Sample: {} user(s), {} group(s) found.", users.len(), groups.len()))
        }
        Connector::EntraId { .. } => {
            let tok = entra::token(c).await?;
            let v: Value = entra::get(&tok, "https://graph.microsoft.com/v1.0/organization?$select=displayName").await?;
            Ok(format!("Connected to Microsoft Graph for {}.", v["value"][0]["displayName"].as_str().unwrap_or("your tenant")))
        }
        Connector::Webhook { .. } | Connector::Slack { .. } | Connector::Teams { .. } => {
            let e = AuditEntry {
                id: 0,
                at: Utc::now(),
                actor: "OpenDirectory".into(),
                action: "integration.test".into(),
                target: "Test message".into(),
                details: "If you can read this, the integration works.".into(),
            };
            send(c, &e).await?;
            Ok("Test event delivered.".into())
        }
        Connector::WindowsService { service, host, port } => {
            let p = port.or(default_port(service)).unwrap_or(445);
            if service == "dhcp" {
                // DHCP is UDP broadcast; check the name resolves instead.
                let addrs = tokio::net::lookup_host((host.as_str(), p)).await?;
                return Ok(format!(
                    "{host} resolves to {}. DHCP itself uses UDP broadcast, which can't be tested from here.",
                    addrs.map(|a| a.ip().to_string()).collect::<Vec<_>>().join(", ")
                ));
            }
            let started = std::time::Instant::now();
            tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect((host.as_str(), p)))
                .await
                .map_err(|_| anyhow::anyhow!("{host}:{p} didn't answer within 5 seconds (firewall or wrong host?)"))??;
            Ok(format!("{host}:{p} is reachable ({} ms).", started.elapsed().as_millis()))
        }
    }
}

// ------------------------------------------------------------------ import

#[derive(Debug, Clone, Default)]
pub struct ImportedUser {
    pub username: String,
    pub display_name: String,
    pub email: String,
    pub title: String,
    pub department: String,
    pub enabled: bool,
    /// Source-side group keys (DN or object id).
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ImportedGroup {
    pub key: String,
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Default)]
pub struct ImportSummary {
    users_created: u32,
    users_updated: u32,
    users_skipped: Vec<String>,
    groups_created: u32,
    memberships: u32,
    ou: String,
    note: String,
}

/// Groups that exist in every AD domain and would only add noise.
const SKIP_GROUPS: &[&str] = &[
    "Domain Users",
    "Domain Computers",
    "Domain Controllers",
    "Domain Guests",
    "Schema Admins",
    "Enterprise Admins",
    "Cert Publishers",
    "Group Policy Creator Owners",
    "RAS and IAS Servers",
    "Allowed RODC Password Replication Group",
    "Denied RODC Password Replication Group",
    "Read-only Domain Controllers",
    "Enterprise Read-only Domain Controllers",
    "Cloneable Domain Controllers",
    "Protected Users",
    "Key Admins",
    "Enterprise Key Admins",
    "DnsAdmins",
    "DnsUpdateProxy",
    "Domain Admins",
];

fn apply_import(s: &AppState, source: &str, users: Vec<ImportedUser>, groups: Vec<ImportedGroup>) -> ApiResult<ImportSummary> {
    let mut sum = ImportSummary::default();
    let now = Utc::now();
    let ou_name = format!("Imported from {source}");
    let ou_id = match s.db.list_ous()?.into_iter().find(|o| o.name == ou_name && o.parent_id.is_none()) {
        Some(o) => o.id,
        None => {
            let o = OrgUnit {
                id: Uuid::new_v4(),
                name: ou_name.clone(),
                description: format!("Users and groups imported from {source}"),
                parent_id: None,
                created_at: now,
            };
            s.db.insert_ou(&o)?;
            o.id
        }
    };
    sum.ou = ou_name;

    let mut existing_groups: HashMap<String, Group> = s.db.list_groups()?.into_iter().map(|g| (g.name.to_lowercase(), g)).collect();
    let mut key_to_group: HashMap<String, Uuid> = HashMap::new();
    for g in groups {
        if SKIP_GROUPS.iter().any(|x| x.eq_ignore_ascii_case(&g.name)) || g.name.trim().is_empty() {
            continue;
        }
        let id = match existing_groups.get(&g.name.to_lowercase()) {
            Some(e) => e.id,
            None => {
                let ng = Group {
                    id: Uuid::new_v4(),
                    name: g.name.clone(),
                    description: g.description.clone(),
                    ou_id: Some(ou_id),
                    member_ids: vec![],
                    endpoint_ids: vec![],
                    created_at: now,
                };
                s.db.insert_group(&ng)?;
                sum.groups_created += 1;
                existing_groups.insert(g.name.to_lowercase(), ng.clone());
                ng.id
            }
        };
        key_to_group.insert(g.key, id);
    }

    let domain_users = existing_groups.get("domain users").map(|g| g.id);
    let mut members: HashMap<Uuid, HashSet<Uuid>> = HashMap::new();
    for u in users {
        let Ok(name) = validate_username(&u.username) else {
            sum.users_skipped.push(format!("{} (name not allowed)", u.username));
            continue;
        };
        let id = match s.db.get_user_by_username(&name)? {
            Some(mut e) => {
                if e.role == Role::Admin {
                    sum.users_skipped.push(format!("{name} (an Admin Center account; not changed)"));
                    continue;
                }
                e.display_name = if u.display_name.is_empty() { e.display_name } else { u.display_name.clone() };
                e.email = u.email.clone();
                e.title = u.title.clone();
                e.department = u.department.clone();
                e.enabled = u.enabled;
                s.db.update_user(&e)?;
                sum.users_updated += 1;
                e.id
            }
            None => {
                let nu = User {
                    id: Uuid::new_v4(),
                    username: name.clone(),
                    display_name: if u.display_name.is_empty() { name.clone() } else { u.display_name.clone() },
                    email: u.email.clone(),
                    title: u.title.clone(),
                    department: u.department.clone(),
                    ou_id: Some(ou_id),
                    enabled: u.enabled,
                    role: Role::User,
                    must_change_password: true,
                    password_set_at: None,
                    last_login_at: None,
                    created_at: now,
                    group_ids: vec![],
                };
                s.db.insert_user(&nu)?;
                sum.users_created += 1;
                nu.id
            }
        };
        if let Some(du) = domain_users {
            members.entry(du).or_default().insert(id);
        }
        for k in &u.groups {
            if let Some(g) = key_to_group.get(k) {
                members.entry(*g).or_default().insert(id);
            }
        }
    }
    for (gid, add) in members {
        let Some(g) = s.db.get_group(gid)? else { continue };
        let mut set: HashSet<Uuid> = g.member_ids.into_iter().collect();
        let before = set.len();
        set.extend(add);
        sum.memberships += (set.len() - before) as u32;
        s.db.set_group_members(gid, &set.into_iter().collect::<Vec<_>>())?;
    }
    sum.note = "Passwords can't be read from the source directory, so new users need a password set (Users > Reset password) before they can sign in.".into();
    Ok(sum)
}

pub async fn import(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<ImportSummary>> {
    let mut x = load(&s, id)?;
    let fetched = match &x.connector {
        Connector::ActiveDirectory { .. } => ad::fetch(&x.connector, None).await,
        Connector::EntraId { .. } => entra::fetch(&x.connector).await,
        _ => return Err(ApiError::bad_request("This integration doesn't import users")),
    };
    let result = fetched.map_err(|e| ApiError::bad_request(format!("{e:#}"))).and_then(|(u, g)| apply_import(&s, &x.name, u, g));
    x.last_sync_at = Some(Utc::now());
    match &result {
        Ok(sum) => {
            x.last_ok = Some(true);
            x.last_message = format!(
                "{} created, {} updated, {} groups created, {} skipped",
                sum.users_created,
                sum.users_updated,
                sum.groups_created,
                sum.users_skipped.len()
            );
        }
        Err(e) => {
            x.last_ok = Some(false);
            x.last_message = e.message.clone();
        }
    }
    save(&s, &x)?;
    s.db.audit(admin.name(), "integration.import", &x.name, &x.last_message);
    Ok(Json(result?))
}

// ------------------------------------------------------------------ events

async fn send(c: &Connector, e: &AuditEntry) -> anyhow::Result<()> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?;
    let text =
        format!("{}: {} {}{}", e.actor, e.action, e.target, if e.details.is_empty() { String::new() } else { format!(" ({})", e.details) });
    let resp = match c {
        Connector::Webhook { url, secret, .. } => {
            let body = serde_json::to_vec(
                &json!({ "event": e.action, "at": e.at, "actor": e.actor, "target": e.target, "details": e.details, "id": e.id }),
            )?;
            let mut rq = client
                .post(url)
                .header("content-type", "application/json")
                .header("user-agent", concat!("OpenDirectory/", env!("CARGO_PKG_VERSION")));
            if !secret.is_empty() {
                use hmac::{Hmac, Mac};
                let mut m = <Hmac<sha2::Sha256> as Mac>::new_from_slice(secret.as_bytes()).expect("any key");
                m.update(&body);
                rq = rq.header("x-opendirectory-signature", format!("sha256={}", hex::encode(m.finalize().into_bytes())));
            }
            rq.body(body).send().await?
        }
        Connector::Slack { webhook_url, .. } => {
            client.post(webhook_url).json(&json!({ "text": format!(":shield: {text}") })).send().await?
        }
        Connector::Teams { webhook_url, .. } => client.post(webhook_url).json(&json!({ "text": text })).send().await?,
        _ => return Ok(()),
    };
    if !resp.status().is_success() {
        anyhow::bail!("The receiver answered {}", resp.status());
    }
    Ok(())
}

/// Forward audit events to webhooks, Slack and Teams. Runs for the server's lifetime.
pub fn spawn_dispatcher(s: AppState) {
    let Some(mut rx) = s.db.subscribe() else { return };
    tokio::spawn(async move {
        let mut cache: (std::time::Instant, Vec<Stored>) = (std::time::Instant::now() - Duration::from_secs(3600), vec![]);
        while let Some(e) = rx.recv().await {
            if e.action.starts_with("integration.") && e.action != "integration.test" {
                cache.0 = std::time::Instant::now() - Duration::from_secs(3600);
                // config changed
            }
            if cache.0.elapsed() > Duration::from_secs(30) {
                cache = (std::time::Instant::now(), load_all(&s).unwrap_or_default());
            }
            for x in cache.1.iter().filter(|x| x.enabled && x.connector.wants(&e.action)) {
                let c = x.connector.clone();
                let e = e.clone();
                tokio::spawn(async move {
                    if let Err(err) = send(&c, &e).await {
                        tracing::warn!("integration delivery failed: {err:#}");
                    }
                });
            }
        }
    });
}

// ------------------------------------------------------------------ Active Directory (LDAP)
mod ad {
    use super::*;
    use ldap3::adapters::{Adapter, EntriesOnly, PagedResults};
    use ldap3::{LdapConnAsync, LdapConnSettings, Scope, SearchEntry};

    fn first(e: &SearchEntry, a: &str) -> String {
        e.attrs.get(a).and_then(|v| v.first()).cloned().unwrap_or_default()
    }

    pub async fn fetch(c: &Connector, limit: Option<usize>) -> anyhow::Result<(Vec<ImportedUser>, Vec<ImportedGroup>)> {
        let Connector::ActiveDirectory { url, bind_dn, password, base_dn, starttls, skip_tls_verify } = c else { unreachable!() };
        let _ = rustls::crypto::ring::default_provider().install_default();
        let settings =
            LdapConnSettings::new().set_conn_timeout(Duration::from_secs(10)).set_starttls(*starttls).set_no_tls_verify(*skip_tls_verify);
        let (conn, mut ldap) =
            LdapConnAsync::with_settings(settings, url).await.map_err(|e| anyhow::anyhow!("Connecting to {url}: {e}"))?;
        ldap3::drive!(conn);
        ldap.simple_bind(bind_dn, password)
            .await?
            .success()
            .map_err(|e| anyhow::anyhow!("The domain controller rejected the bind account: {e}"))?;

        let search = |filter: &'static str, attrs: Vec<&'static str>| {
            let mut ldap = ldap.clone();
            let base = base_dn.clone();
            async move {
                let adapters: Vec<Box<dyn Adapter<_, _>>> = vec![Box::new(EntriesOnly::new()), Box::new(PagedResults::new(500))];
                let mut s = ldap.streaming_search_with(adapters, &base, Scope::Subtree, filter, attrs).await?;
                let mut out = vec![];
                while let Some(e) = s.next().await? {
                    out.push(SearchEntry::construct(e));
                    if limit.map(|l| out.len() >= l).unwrap_or(false) {
                        break;
                    }
                }
                let _ = s.finish().await;
                anyhow::Ok(out)
            }
        };
        let users = search(
            "(&(objectCategory=person)(objectClass=user))",
            vec!["sAMAccountName", "displayName", "mail", "title", "department", "userAccountControl", "memberOf"],
        )
        .await?;
        let groups = search("(objectClass=group)", vec!["cn", "description", "groupType"]).await?;
        let _ = ldap.unbind().await;

        let users = users
            .into_iter()
            .filter(|e| !first(e, "sAMAccountName").ends_with('$'))
            .map(|e| ImportedUser {
                username: first(&e, "sAMAccountName"),
                display_name: first(&e, "displayName"),
                email: first(&e, "mail"),
                title: first(&e, "title"),
                department: first(&e, "department"),
                // userAccountControl bit 2 = ACCOUNTDISABLE
                enabled: first(&e, "userAccountControl").parse::<u32>().map(|v| v & 2 == 0).unwrap_or(true),
                groups: e.attrs.get("memberOf").cloned().unwrap_or_default().into_iter().map(|d| d.to_lowercase()).collect(),
            })
            .collect();
        let groups = groups
            .into_iter()
            // Only security groups (groupType has the 0x80000000 bit).
            .filter(|e| first(e, "groupType").parse::<i64>().map(|t| t < 0).unwrap_or(true))
            .map(|e| ImportedGroup { key: e.dn.to_lowercase(), name: first(&e, "cn"), description: first(&e, "description") })
            .collect();
        Ok((users, groups))
    }
}

// ------------------------------------------------------------------ Entra ID (Microsoft Graph)
mod entra {
    use super::*;

    pub async fn token(c: &Connector) -> anyhow::Result<String> {
        let Connector::EntraId { tenant_id, client_id, client_secret } = c else { unreachable!() };
        let r = reqwest::Client::new()
            .post(format!("https://login.microsoftonline.com/{}/oauth2/v2.0/token", tenant_id.trim()))
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", client_id.trim()),
                ("client_secret", client_secret.as_str()),
                ("scope", "https://graph.microsoft.com/.default"),
            ])
            .send()
            .await?;
        let status = r.status();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            anyhow::bail!(
                "Microsoft sign-in failed: {}",
                v["error_description"].as_str().unwrap_or("check the tenant ID, client ID and secret")
            );
        }
        v["access_token"].as_str().map(str::to_string).ok_or_else(|| anyhow::anyhow!("No access token returned"))
    }

    pub async fn get(tok: &str, url: &str) -> anyhow::Result<Value> {
        let r = reqwest::Client::new().get(url).bearer_auth(tok).send().await?;
        let status = r.status();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            anyhow::bail!(
                "Microsoft Graph answered {status}: {}",
                v["error"]["message"]
                    .as_str()
                    .unwrap_or("grant the app User.Read.All and GroupMember.Read.All (application) permissions with admin consent")
            );
        }
        Ok(v)
    }

    async fn all(tok: &str, mut url: String) -> anyhow::Result<Vec<Value>> {
        let mut out = vec![];
        loop {
            let v = get(tok, &url).await?;
            out.extend(v["value"].as_array().cloned().unwrap_or_default());
            match v["@odata.nextLink"].as_str() {
                Some(n) => url = n.to_string(),
                None => break,
            }
            if out.len() > 100_000 {
                break;
            }
        }
        Ok(out)
    }

    pub async fn fetch(c: &Connector) -> anyhow::Result<(Vec<ImportedUser>, Vec<ImportedGroup>)> {
        let tok = token(c).await?;
        let users = all(&tok, "https://graph.microsoft.com/v1.0/users?$select=id,userPrincipalName,mailNickname,displayName,mail,jobTitle,department,accountEnabled&$top=999".into()).await?;
        let groups = all(
            &tok,
            "https://graph.microsoft.com/v1.0/groups?$filter=securityEnabled eq true&$select=id,displayName,description&$top=999".into(),
        )
        .await?;
        let mut user_groups: HashMap<String, Vec<String>> = HashMap::new();
        for g in &groups {
            let gid = g["id"].as_str().unwrap_or("").to_string();
            for m in all(&tok, format!("https://graph.microsoft.com/v1.0/groups/{gid}/members/microsoft.graph.user?$select=id&$top=999"))
                .await
                .unwrap_or_default()
            {
                if let Some(uid) = m["id"].as_str() {
                    user_groups.entry(uid.to_string()).or_default().push(gid.clone());
                }
            }
        }
        let s = |v: &Value, k: &str| v[k].as_str().unwrap_or("").to_string();
        Ok((
            users
                .iter()
                .map(|u| {
                    let nick = s(u, "mailNickname");
                    let upn_local = s(u, "userPrincipalName").split('@').next().unwrap_or("").to_string();
                    ImportedUser {
                        username: if nick.is_empty() { upn_local } else { nick }.to_lowercase(),
                        display_name: s(u, "displayName"),
                        email: s(u, "mail"),
                        title: s(u, "jobTitle"),
                        department: s(u, "department"),
                        enabled: u["accountEnabled"].as_bool().unwrap_or(true),
                        groups: user_groups.get(&s(u, "id")).cloned().unwrap_or_default(),
                    }
                })
                .collect(),
            groups.iter().map(|g| ImportedGroup { key: s(g, "id"), name: s(g, "displayName"), description: s(g, "description") }).collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_redacted_and_kept() {
        let c = Connector::Slack { webhook_url: "https://hooks.slack.com/services/T/B/x".into(), events: vec![] };
        let r = c.redacted();
        assert!(!serde_json::to_string(&r).unwrap().contains("hooks.slack.com"));
        let mut back = r.clone();
        back.merge_secrets(&c);
        assert!(serde_json::to_string(&back).unwrap().contains("hooks.slack.com"));
        let w = Connector::Webhook { url: "https://x".into(), secret: String::new(), events: vec!["backup.".into()] };
        assert!(w.wants("backup.failed") && !w.wants("user.create"));
    }
}
