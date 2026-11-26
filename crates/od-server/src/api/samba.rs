//! Domain Controller role, backed by Samba AD on a Linux Management Server.
//!
//! Samba provides the real Active Directory protocols (Kerberos, LDAP,
//! SMB, Group Policy, domain join for Windows Pro/Enterprise, replication,
//! sites, trusts and FSMO roles). OpenDirectory provisions it, keeps its
//! users, passwords and groups in sync with the OpenDirectory directory,
//! and wraps the samba-tool commands for sites, FSMO, trusts and
//! additional domain controllers.
//!
//! This needs `samba` (with its Python bindings) installed and the
//! Management Server running as root. It conflicts with OpenDirectory's own
//! DNS and LDAP servers, which must be turned off first.

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::Admin;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

const SAM: &str = "/var/lib/samba/private/sam.ldb";

#[derive(Serialize, Default)]
pub struct DcStatus {
    supported_os: bool,
    samba_installed: bool,
    samba_version: String,
    provisioned: bool,
    running: bool,
    realm: String,
    last_sync: Option<String>,
    last_sync_message: String,
    conflicts: Vec<String>,
}

async fn run(program: &str, args: &[&str], stdin: Option<&str>, timeout: u64) -> anyhow::Result<(bool, String)> {
    use tokio::io::AsyncWriteExt;
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).stdin(if stdin.is_some() {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::null()
    });
    let mut child = cmd.spawn()?;
    if let Some(input) = stdin {
        if let Some(mut si) = child.stdin.take() {
            si.write_all(input.as_bytes()).await?;
        }
    }
    let out = tokio::time::timeout(Duration::from_secs(timeout), child.wait_with_output()).await??;
    Ok((
        out.status.success(),
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)).trim().to_string(),
    ))
}

fn conflicts(s: &AppState) -> Vec<String> {
    let mut v = vec![];
    if crate::netsvc::dns_config(s).enabled {
        v.push(
            "OpenDirectory's DNS server is on. Samba runs its own DNS for the domain; turn OpenDirectory's off in Network Services.".into(),
        );
    }
    if crate::netsvc::ldap_config(s).enabled {
        v.push("OpenDirectory's LDAP server is on. Samba serves LDAP on 389/636; turn OpenDirectory's off in Network Services.".into());
    }
    v
}

pub async fn status(State(s): State<AppState>, _a: Admin) -> Json<DcStatus> {
    let mut st = DcStatus { supported_os: cfg!(target_os = "linux"), conflicts: conflicts(&s), ..Default::default() };
    if !st.supported_os {
        return Json(st);
    }
    if let Ok((ok, v)) = run("samba-tool", &["--version"], None, 10).await {
        st.samba_installed = ok;
        st.samba_version = v;
    }
    st.provisioned = std::path::Path::new(SAM).exists();
    st.running = run("systemctl", &["is-active", "samba-ad-dc"], None, 10).await.map(|(ok, _)| ok).unwrap_or(false);
    st.realm = s.db.meta_get("samba_realm").ok().flatten().unwrap_or_default();
    st.last_sync = s.db.meta_get("samba_last_sync").ok().flatten();
    st.last_sync_message = s.db.meta_get("samba_last_sync_message").ok().flatten().unwrap_or_default();
    Json(st)
}

#[derive(Deserialize)]
pub struct ProvisionInput {
    admin_password: String,
    #[serde(default = "fwd")]
    dns_forwarder: String,
}
fn fwd() -> String {
    "1.1.1.1".into()
}

/// Provision a new Samba AD domain matching this directory's domain.
pub async fn provision(State(s): State<AppState>, admin: Admin, Json(inp): Json<ProvisionInput>) -> ApiResult<Json<serde_json::Value>> {
    if !cfg!(target_os = "linux") {
        return Err(ApiError::bad_request("The domain controller role needs a Linux Management Server with Samba"));
    }
    if let Some(c) = conflicts(&s).first() {
        return Err(ApiError::conflict(c.clone()));
    }
    if std::path::Path::new(SAM).exists() {
        return Err(ApiError::conflict("Samba is already provisioned on this server"));
    }
    if inp.admin_password.len() < 12 {
        return Err(ApiError::bad_request("Use an Administrator password of at least 12 characters with mixed character types"));
    }
    inp.dns_forwarder.parse::<std::net::IpAddr>().map_err(|_| ApiError::bad_request("The DNS forwarder must be an IP address"))?;
    let domain = s.db.meta_get("domain")?.ok_or_else(|| ApiError::bad_request("Set up the domain first"))?;
    let realm = domain.to_uppercase();
    let netbios: String =
        domain.split('.').next().unwrap_or("CORP").to_uppercase().chars().filter(|c| c.is_ascii_alphanumeric()).take(15).collect();
    // samba-tool takes the password as an option; read it from a 0600 file so
    // it doesn't appear in the process list.
    let pw_file = std::env::temp_dir().join(format!("od-samba-{}", uuid::Uuid::new_v4()));
    crate::crypto::write_private(&pw_file, inp.admin_password.as_bytes())?;
    let script = format!(
        "set -e\nsamba-tool domain provision --server-role=dc --use-rfc2307 --dns-backend=SAMBA_INTERNAL --realm={realm} --domain={netbios} --adminpass=\"$(cat '{}')\" --option='dns forwarder = {}'\ncp /var/lib/samba/private/krb5.conf /etc/krb5.conf\nsystemctl disable --now smbd nmbd winbind 2>/dev/null || true\nsystemctl unmask samba-ad-dc 2>/dev/null || true\nsystemctl enable --now samba-ad-dc\n",
        pw_file.display(),
        inp.dns_forwarder
    );
    let res = run("sh", &["-c", &script], None, 900).await;
    let _ = std::fs::remove_file(&pw_file);
    let (ok, out) = res.map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    let tail: String = out.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    if !ok {
        return Err(ApiError::bad_request(format!("Provisioning failed:\n{tail}")));
    }
    s.db.meta_set("samba_realm", &realm)?;
    s.db.audit(admin.name(), "dc.provision", &realm, "Samba AD domain controller provisioned");
    let msg = sync_now(&s).await;
    Ok(Json(serde_json::json!({ "ok": true, "output": tail, "sync": msg })))
}

/// Python run with Samba's bindings: reads the desired users and groups as
/// JSON on stdin (so passwords never appear on a command line) and makes
/// Samba match. Only touches objects marked as managed by OpenDirectory.
const SYNC_PY: &str = r#"
import json, sys
from samba.samdb import SamDB
from samba.auth import system_session
from samba.param import LoadParm
from samba import dsdb
import ldb

want = json.load(sys.stdin)
lp = LoadParm()
if want.get("smbconf"):
    lp.load(want["smbconf"])
else:
    lp.load_default()
db = SamDB(url=want["sam"], session_info=system_session(), lp=lp)
base = db.domain_dn()
MARK = "OpenDirectory"
out = {"created": 0, "updated": 0, "passwords": 0, "disabled": 0, "groups": 0, "errors": []}

def val(e, a):
    v = e.get(a)
    if v is None or len(v) == 0:
        return ""
    x = v[0]
    return x.decode() if isinstance(x, bytes) else str(x)

def find(cls, name):
    r = db.search(base, scope=ldb.SCOPE_SUBTREE, expression="(&(objectClass=%s)(sAMAccountName=%s))" % (cls, ldb.binary_encode(name)), attrs=["info", "userAccountControl"])
    return r[0] if len(r) else None

for u in want["users"]:
    try:
        e = find("user", u["username"])
        if e is None:
            db.newuser(u["username"], u["password"] or None, givenname=u["given"] or None, surname=u["surname"] or None, mailaddress=u["mail"] or None, setpassword=bool(u["password"]))
            m = ldb.Message(); m.dn = find("user", u["username"]).dn
            m["info"] = ldb.MessageElement(MARK, ldb.FLAG_MOD_REPLACE, "info")
            db.modify(m); out["created"] += 1
            if not u["password"]:
                db.disable_account("(sAMAccountName=%s)" % ldb.binary_encode(u["username"]))
        elif val(e, "info") != MARK:
            continue  # created outside OpenDirectory: leave it alone
        else:
            if u["password"]:
                db.setpassword("(sAMAccountName=%s)" % ldb.binary_encode(u["username"]), u["password"], force_change_at_next_login=False)
                out["passwords"] += 1
            if u["enabled"]:
                db.enable_account("(sAMAccountName=%s)" % ldb.binary_encode(u["username"]))
            else:
                db.disable_account("(sAMAccountName=%s)" % ldb.binary_encode(u["username"])); out["disabled"] += 1
            out["updated"] += 1
    except Exception as ex:
        out["errors"].append("%s: %s" % (u["username"], ex))

managed = set(u["username"].lower() for u in want["users"])
for e in db.search(base, scope=ldb.SCOPE_SUBTREE, expression="(&(objectClass=user)(info=%s))" % MARK, attrs=["sAMAccountName"]):
    n = val(e, "sAMAccountName")
    if n.lower() not in managed:
        db.disable_account("(sAMAccountName=%s)" % ldb.binary_encode(n)); out["disabled"] += 1

for g in want["groups"]:
    try:
        e = find("group", g["name"])
        if e is None:
            db.newgroup(g["name"], description=g["description"] or None)
            m = ldb.Message(); m.dn = find("group", g["name"]).dn
            m["info"] = ldb.MessageElement(MARK, ldb.FLAG_MOD_REPLACE, "info"); db.modify(m)
        current = set()
        for r in db.search(base, scope=ldb.SCOPE_SUBTREE, expression="(&(objectClass=user)(memberOf=%s))" % ldb.binary_encode(str(find("group", g["name"]).dn)), attrs=["sAMAccountName"]):
            current.add(val(r, "sAMAccountName").lower())
        add = [m for m in g["members"] if m.lower() not in current]
        rm = [m for m in current if m in managed and m not in [x.lower() for x in g["members"]]]
        if add: db.add_remove_group_members(g["name"], add, add_members_operation=True)
        if rm: db.add_remove_group_members(g["name"], rm, add_members_operation=False)
        out["groups"] += 1
    except Exception as ex:
        out["errors"].append("group %s: %s" % (g["name"], ex))
print(json.dumps(out))
"#;

/// Push users, passwords (only those changed since the last sync) and groups to Samba.
pub async fn sync_now(s: &AppState) -> String {
    if !std::path::Path::new(SAM).exists() {
        return "Samba isn't provisioned".into();
    }
    sync_with(s, SAM, None).await
}

/// Sync into a given sam.ldb (and smb.conf); tests use a throwaway domain.
pub(crate) async fn sync_with(s: &AppState, sam: &str, smbconf: Option<&str>) -> String {
    let synced: HashMap<String, i64> =
        s.db.meta_get("samba_pw_versions").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
    let mut versions = HashMap::new();
    let users = s.db.list_users().unwrap_or_default();
    let mut want_users = vec![];
    for u in &users {
        let sec = match s.db.user_secrets(u.id) {
            Ok(x) => x,
            Err(_) => continue,
        };
        let pw = if synced.get(&u.username) != Some(&sec.password_version) {
            sec.password_sealed.as_ref().and_then(|b| s.sealer.open(b).ok()).and_then(|b| String::from_utf8(b).ok()).unwrap_or_default()
        } else {
            String::new()
        };
        versions.insert(u.username.clone(), sec.password_version);
        let (given, surname) =
            u.display_name.split_once(' ').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((u.display_name.clone(), String::new()));
        want_users.push(serde_json::json!({ "username": u.username, "password": pw, "enabled": u.enabled, "given": given, "surname": surname, "mail": u.email }));
    }
    let map: HashMap<uuid::Uuid, String> = users.iter().map(|u| (u.id, u.username.clone())).collect();
    let want_groups: Vec<serde_json::Value> = s
        .db
        .list_groups()
        .unwrap_or_default()
        .into_iter()
        .filter(|g| g.name != "Domain Users")
        .map(|g| serde_json::json!({ "name": g.name, "description": g.description, "members": g.member_ids.iter().filter_map(|m| map.get(m)).collect::<Vec<_>>() }))
        .collect();
    let input = serde_json::json!({ "sam": sam, "smbconf": smbconf, "users": want_users, "groups": want_groups }).to_string();
    let msg = match run("python3", &["-c", SYNC_PY], Some(&input), 600).await {
        Ok((true, out)) => {
            let _ = s.db.meta_set("samba_pw_versions", &serde_json::to_string(&versions).unwrap_or_default());
            let v: serde_json::Value = serde_json::from_str(out.lines().last().unwrap_or("{}")).unwrap_or_default();
            format!(
                "{} created, {} updated, {} password(s) synced, {} disabled, {} groups{}",
                v["created"],
                v["updated"],
                v["passwords"],
                v["disabled"],
                v["groups"],
                v["errors"]
                    .as_array()
                    .filter(|e| !e.is_empty())
                    .map(|e| format!(
                        "; {} error(s): {}",
                        e.len(),
                        e.iter().filter_map(|x| x.as_str()).take(3).collect::<Vec<_>>().join("; ")
                    ))
                    .unwrap_or_default()
            )
        }
        Ok((false, out)) => format!("Sync failed: {}", out.lines().last().unwrap_or("")),
        Err(e) => format!("Sync failed: {e:#}"),
    };
    let _ = s.db.meta_set("samba_last_sync", &chrono::Utc::now().to_rfc3339());
    let _ = s.db.meta_set("samba_last_sync_message", &msg);
    msg
}

pub async fn sync(State(s): State<AppState>, admin: Admin) -> Json<serde_json::Value> {
    let msg = sync_now(&s).await;
    s.db.audit(admin.name(), "dc.sync", "", &msg);
    Json(serde_json::json!({ "message": msg }))
}

/// Read-only samba-tool views shown on the Domain Controller page.
pub async fn info(State(_s): State<AppState>, _a: Admin) -> Json<serde_json::Value> {
    let q = |args: &'static [&'static str]| async move {
        run("samba-tool", args, None, 30).await.map(|(_, o)| o).unwrap_or_else(|e| format!("{e:#}"))
    };
    Json(serde_json::json!({
        "fsmo": q(&["fsmo", "show"]).await,
        "sites": q(&["sites", "list"]).await,
        "trusts": q(&["domain", "trust", "list"]).await,
        "replication": q(&["drs", "showrepl", "--summary"]).await,
        "level": q(&["domain", "level", "show"]).await,
    }))
}

#[derive(Deserialize)]
pub struct ToolInput {
    /// One of the allowed actions below.
    action: String,
    #[serde(default)]
    args: Vec<String>,
}

/// Run one of a short list of samba-tool administrative actions.
pub async fn action(State(s): State<AppState>, admin: Admin, Json(inp): Json<ToolInput>) -> ApiResult<Json<serde_json::Value>> {
    let safe = |a: &str| {
        !a.is_empty() && !a.starts_with('-') && a.len() < 200 && a.chars().all(|c| c.is_ascii_alphanumeric() || "._-/@ ".contains(c))
    };
    if !inp.args.iter().all(|a| safe(a)) {
        return Err(ApiError::bad_request("Arguments may only use letters, numbers and . _ - / @"));
    }
    let a = |i: usize| inp.args.get(i).cloned().unwrap_or_default();
    let args: Vec<String> = match inp.action.as_str() {
        "site_create" => vec!["sites".into(), "create".into(), a(0)],
        "subnet_create" => vec!["sites".into(), "subnet".into(), "create".into(), a(0), a(1)],
        "fsmo_transfer" => vec!["fsmo".into(), "transfer".into(), format!("--role={}", a(0))],
        "fsmo_seize" => vec!["fsmo".into(), "seize".into(), format!("--role={}", a(0)), "--force".into()],
        "replicate" => vec!["drs".into(), "replicate".into(), a(0), a(1), a(2)],
        _ => return Err(ApiError::bad_request("Unknown action")),
    };
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (ok, out) = run("samba-tool", &refs, None, 300).await.map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    s.db.audit(admin.name(), "dc.action", &inp.action, &inp.args.join(" "));
    Ok(Json(serde_json::json!({ "ok": ok, "output": out })))
}

/// Keep Samba in step with the directory every five minutes.
pub fn spawn_sync(s: AppState) {
    if !cfg!(target_os = "linux") {
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(300)).await;
            if std::path::Path::new(SAM).exists() && s.db.meta_get("samba_realm").ok().flatten().is_some() {
                let _ = sync_now(&s).await;
            }
        }
    });
}
