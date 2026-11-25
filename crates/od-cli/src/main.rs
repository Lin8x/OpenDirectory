//! `od`: the OpenDirectory command-line interface. Every command maps to
//! the same management API the Admin Center uses.

mod glossary;

use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use od_client::{Client, ServerTarget};
use od_core::policy::Policy;
use od_core::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "od", version, about = "OpenDirectory command-line interface")]
struct Cli {
    /// Print raw JSON instead of tables.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Sign in to a Management Server.
    Login {
        /// e.g. https://10.0.0.4:8443
        #[arg(long)]
        server: String,
        #[arg(long)]
        fingerprint: Option<String>,
        #[arg(long, short)]
        username: String,
    },
    /// Sign out and forget the saved session.
    Logout,
    /// Domain overview: counts, health and getting-started progress.
    Status,
    /// Search users, groups, endpoints, OUs and GPOs.
    Search { query: String },
    /// Manage user accounts.
    #[command(subcommand)]
    User(UserCmd),
    /// Manage security groups.
    #[command(subcommand)]
    Group(GroupCmd),
    /// List organizational units as a tree.
    Ou,
    /// Manage endpoints.
    #[command(subcommand)]
    Endpoint(EndpointCmd),
    /// Create an enrollment token and print the enroll commands.
    Enroll {
        #[arg(long, default_value = "")]
        description: String,
        /// Put new endpoints in this OU.
        #[arg(long)]
        ou: Option<String>,
        /// Maximum number of computers that can use it.
        #[arg(long)]
        uses: Option<u32>,
    },
    /// List Group Policy Objects, or show one.
    Gpo { name: Option<String> },
    /// Show the audit log.
    Audit {
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long)]
        filter: Option<String>,
    },
    /// Explain a directory term in plain language (e.g. `od explain gpo`).
    Explain { term: Option<String> },
}

#[derive(Subcommand)]
enum UserCmd {
    List,
    Show { username: String },
    Add {
        username: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        email: Option<String>,
        /// Make this user a Domain Admin.
        #[arg(long)]
        admin: bool,
        /// Add to these security groups (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
    },
    Disable { username: String },
    Enable { username: String },
    /// Reset a password (synced to endpoints on their next check-in).
    Passwd { username: String },
    Delete { username: String },
}

#[derive(Subcommand)]
enum GroupCmd {
    List,
    Add {
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    Members { group: String },
    AddMember { group: String, username: String },
    RemoveMember { group: String, username: String },
}

#[derive(Subcommand)]
enum EndpointCmd {
    List,
    Show { hostname: String },
    /// Run a script and wait for the output.
    Run {
        hostname: String,
        /// default | bash | zsh | powershell | cmd
        #[arg(long, default_value = "default")]
        shell: String,
        /// The script to run.
        script: String,
        /// Don't wait for the result.
        #[arg(long)]
        no_wait: bool,
    },
    /// Show the Resultant Set of Policy for an endpoint.
    Rsop { hostname: String },
    /// Revoke an endpoint's agent credentials and remove it.
    Retire { hostname: String },
}

#[derive(Serialize, Deserialize)]
struct Config {
    server: ServerTarget,
    token: String,
    username: String,
}

fn config_path() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."))
    } else {
        std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config"))
    };
    base.join("opendirectory").join("cli.json")
}

fn load_config() -> Result<Config> {
    let p = config_path();
    let raw = std::fs::read_to_string(&p).map_err(|_| anyhow!("Not signed in. Run `od login --server <url> --username <name>` first."))?;
    Ok(serde_json::from_str(&raw)?)
}

fn save_config(c: &Config) -> Result<()> {
    let p = config_path();
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(&p, serde_json::to_vec_pretty(c)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn client() -> Result<Client> {
    let c = load_config()?;
    Ok(Client::new(&c.server)?.with_session(&c.token))
}

/// Print rows as an aligned table.
fn table(headers: &[&str], rows: Vec<Vec<String>>) {
    let mut w: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for r in &rows {
        for (i, c) in r.iter().enumerate() {
            w[i] = w[i].max(c.chars().count());
        }
    }
    let line = |cells: Vec<String>| {
        let s: Vec<String> = cells.iter().enumerate().map(|(i, c)| format!("{c:<width$}", width = w[i])).collect();
        println!("{}", s.join("  ").trim_end());
    };
    line(headers.iter().map(|h| h.to_uppercase()).collect());
    for r in rows {
        line(r);
    }
}

fn prompt_password(label: &str) -> Result<String> {
    if let Ok(p) = std::env::var("OD_PASSWORD") {
        return Ok(p);
    }
    Ok(rpassword::prompt_password(label)?)
}

fn ago(t: Option<chrono::DateTime<chrono::Utc>>) -> String {
    let Some(t) = t else { return "never".into() };
    let s = (chrono::Utc::now() - t).num_seconds();
    match s {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86400),
    }
}

async fn find_user(c: &Client, name: &str) -> Result<User> {
    let users: Vec<User> = c.get("/users").await?;
    users.into_iter().find(|u| u.username.eq_ignore_ascii_case(name)).ok_or_else(|| anyhow!("No user named {name}"))
}
async fn find_group(c: &Client, name: &str) -> Result<Group> {
    let groups: Vec<Group> = c.get("/groups").await?;
    groups.into_iter().find(|g| g.name.eq_ignore_ascii_case(name)).ok_or_else(|| anyhow!("No group named {name}"))
}
async fn find_endpoint(c: &Client, host: &str) -> Result<Endpoint> {
    let eps: Vec<Endpoint> = c.get("/endpoints").await?;
    let matches: Vec<Endpoint> = eps.into_iter().filter(|e| e.hostname.eq_ignore_ascii_case(host) || e.id.to_string() == host).collect();
    match matches.len() {
        0 => bail!("No endpoint named {host}"),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => bail!("Several endpoints are named {host}; use its ID instead"),
    }
}
async fn find_ou(c: &Client, name: &str) -> Result<OrgUnit> {
    let ous: Vec<OrgUnit> = c.get("/ous").await?;
    ous.into_iter().find(|o| o.name.eq_ignore_ascii_case(name)).ok_or_else(|| anyhow!("No OU named {name}"))
}

fn print_json(v: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    let json_out = cli.json;
    match cli.command {
        Cmd::Login { server, fingerprint, username } => {
            let target = ServerTarget { url: server.trim_end_matches('/').into(), fingerprint };
            let c = Client::new(&target)?;
            let password = prompt_password(&format!("Password for {username}: "))?;
            let resp: Value = c.post("/auth/login", &json!({ "username": username, "password": password })).await?;
            let token = resp["token"].as_str().context("no token in response")?.to_string();
            let name = resp["user"]["username"].as_str().unwrap_or(&username).to_string();
            save_config(&Config { server: target, token, username: name.clone() })?;
            println!("Signed in as {name}.");
        }
        Cmd::Logout => {
            if let Ok(c) = client() {
                let _ = c.request(od_client::Method::POST, "/auth/logout", None).await;
            }
            let _ = std::fs::remove_file(config_path());
            println!("Signed out.");
        }
        Cmd::Status => {
            let c = client()?;
            let v: Value = c.get("/overview").await?;
            if json_out {
                return print_json(&v);
            }
            let n = &v["counts"];
            println!("{}  ({})", v["organization"].as_str().unwrap_or("-"), v["domain"].as_str().unwrap_or("-"));
            println!();
            println!("Users       {} ({} enabled, {} admins)", n["users"], n["users_enabled"], n["admins"]);
            println!("Groups      {}", n["groups"]);
            println!("OUs         {}", n["ous"]);
            println!("Endpoints   {} ({} online, {} non-compliant)", n["endpoints"], n["endpoints_online"], n["endpoints_noncompliant"]);
            println!("GPOs        {}", n["policies"]);
            println!("Jobs        {} queued or running", n["jobs_running"]);
            println!();
            println!("Server      {}", v["server"]["endpoint_url"].as_str().unwrap_or("-"));
        }
        Cmd::Search { query } => {
            let c = client()?;
            let v: Vec<Value> = c.get(&format!("/search?q={}", urlencode(&query))).await?;
            if json_out {
                return print_json(&v);
            }
            table(&["type", "name", "details"], v.iter().map(|r| vec![s(&r["type"]), s(&r["title"]), s(&r["subtitle"])]).collect());
        }
        Cmd::User(cmd) => user(cmd, json_out).await?,
        Cmd::Group(cmd) => group(cmd, json_out).await?,
        Cmd::Ou => {
            let c = client()?;
            let ous: Vec<OrgUnit> = c.get("/ous").await?;
            if json_out {
                return print_json(&ous);
            }
            fn walk(ous: &[OrgUnit], parent: Option<uuid::Uuid>, depth: usize) {
                for o in ous.iter().filter(|o| o.parent_id == parent) {
                    println!("{}{}{}", "  ".repeat(depth), o.name, if o.description.is_empty() { String::new() } else { format!("  ({})", o.description) });
                    walk(ous, Some(o.id), depth + 1);
                }
            }
            walk(&ous, None, 0);
        }
        Cmd::Endpoint(cmd) => endpoint(cmd, json_out).await?,
        Cmd::Enroll { description, ou, uses } => {
            let c = client()?;
            let ou_id = match ou {
                Some(name) => Some(find_ou(&c, &name).await?.id),
                None => None,
            };
            let t: EnrollmentToken = c.post("/enrollment-tokens", &json!({ "description": description, "ou_id": ou_id, "max_uses": uses })).await?;
            let me: Value = c.get("/auth/me").await?;
            let url = me["server"]["endpoint_url"].as_str().unwrap_or("").to_string();
            let fp = me["server"]["tls_fingerprint"].as_str().map(|f| format!(" --fingerprint {f}")).unwrap_or_default();
            if json_out {
                return print_json(&t);
            }
            println!("Enrollment token: {}", t.token);
            if let Some(e) = t.expires_at {
                println!("Expires:          {}", e.format("%Y-%m-%d %H:%M UTC"));
            }
            println!();
            println!("On the computer to enroll, as Administrator/root:");
            println!("  opendirectory-agent enroll --server {url} --token {}{fp} --install", t.token);
        }
        Cmd::Gpo { name } => {
            let c = client()?;
            let ps: Vec<Policy> = c.get("/policies").await?;
            match name {
                None => {
                    if json_out {
                        return print_json(&ps);
                    }
                    table(
                        &["priority", "name", "enabled", "linked to", "settings"],
                        ps.iter()
                            .map(|p| {
                                vec![
                                    p.priority.to_string(),
                                    p.name.clone(),
                                    if p.enabled { "yes".into() } else { "no".into() },
                                    if p.targets.all_endpoints { "All endpoints".into() } else { format!("{} OU(s)", p.targets.ou_ids.len()) },
                                    p.settings.len().to_string(),
                                ]
                            })
                            .collect(),
                    );
                }
                Some(n) => {
                    let p = ps.into_iter().find(|p| p.name.eq_ignore_ascii_case(&n)).ok_or_else(|| anyhow!("No GPO named {n}"))?;
                    if json_out {
                        return print_json(&p);
                    }
                    println!("{} (priority {}, {})", p.name, p.priority, if p.enabled { "enabled" } else { "disabled" });
                    if !p.description.is_empty() {
                        println!("{}", p.description);
                    }
                    println!();
                    for st in &p.settings {
                        println!("  • {}", st.describe());
                    }
                }
            }
        }
        Cmd::Audit { limit, filter } => {
            let c = client()?;
            let mut path = format!("/audit?limit={limit}");
            if let Some(f) = filter {
                path.push_str(&format!("&q={}", urlencode(&f)));
            }
            let a: Vec<AuditEntry> = c.get(&path).await?;
            if json_out {
                return print_json(&a);
            }
            table(
                &["when", "actor", "action", "target", "details"],
                a.iter().map(|e| vec![e.at.format("%Y-%m-%d %H:%M:%S").to_string(), e.actor.clone(), e.action.clone(), e.target.clone(), e.details.clone()]).collect(),
            );
        }
        Cmd::Explain { term } => match term {
            None => {
                for (k, v) in glossary::TERMS {
                    println!("{k}\n  {v}\n");
                }
            }
            Some(t) => match glossary::lookup(&t) {
                Some((k, v)) => println!("{k}\n  {v}"),
                None => bail!("No entry for \"{t}\". Run `od explain` to list every term."),
            },
        },
    }
    Ok(())
}

fn s(v: &Value) -> String {
    v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

async fn user(cmd: UserCmd, json_out: bool) -> Result<()> {
    let c = client()?;
    match cmd {
        UserCmd::List => {
            let users: Vec<User> = c.get("/users").await?;
            if json_out {
                return print_json(&users);
            }
            table(
                &["username", "name", "role", "status", "last sign-in"],
                users
                    .iter()
                    .map(|u| {
                        vec![
                            u.username.clone(),
                            u.display_name.clone(),
                            if u.role == Role::Admin { "Domain Admin".into() } else { "User".into() },
                            if u.enabled { "Enabled".into() } else { "Disabled".into() },
                            ago(u.last_login_at),
                        ]
                    })
                    .collect(),
            );
        }
        UserCmd::Show { username } => {
            let u = find_user(&c, &username).await?;
            if json_out {
                return print_json(&u);
            }
            let groups: Vec<Group> = c.get("/groups").await?;
            let names: Vec<String> = groups.iter().filter(|g| u.group_ids.contains(&g.id)).map(|g| g.name.clone()).collect();
            println!("{} ({})", u.display_name, u.username);
            println!("  Status:      {}", if u.enabled { "Enabled" } else { "Disabled" });
            println!("  Role:        {}", if u.role == Role::Admin { "Domain Admin" } else { "User" });
            println!("  Email:       {}", u.email);
            println!("  Title:       {}", u.title);
            println!("  Department:  {}", u.department);
            println!("  Member of:   {}", names.join(", "));
            println!("  Password:    set {}", ago(u.password_set_at));
        }
        UserCmd::Add { username, name, email, admin, groups } => {
            let password = prompt_password(&format!("Initial password for {username}: "))?;
            let mut gids = vec![];
            for g in groups {
                gids.push(find_group(&c, &g).await?.id);
            }
            let u: User = c
                .post(
                    "/users",
                    &json!({ "username": username, "display_name": name, "email": email, "role": if admin { "admin" } else { "user" }, "password": password, "group_ids": gids }),
                )
                .await?;
            println!("Created {} ({}).", u.username, u.display_name);
        }
        UserCmd::Disable { username } => {
            let u = find_user(&c, &username).await?;
            let _: User = c.patch(&format!("/users/{}", u.id), &json!({ "enabled": false })).await?;
            println!("Disabled {}. Their local accounts on endpoints are locked at the next check-in.", u.username);
        }
        UserCmd::Enable { username } => {
            let u = find_user(&c, &username).await?;
            let _: User = c.patch(&format!("/users/{}", u.id), &json!({ "enabled": true })).await?;
            println!("Enabled {}.", u.username);
        }
        UserCmd::Passwd { username } => {
            let u = find_user(&c, &username).await?;
            let p1 = prompt_password("New password: ")?;
            if std::env::var("OD_PASSWORD").is_err() {
                let p2 = prompt_password("Confirm: ")?;
                if p1 != p2 {
                    bail!("Passwords don't match");
                }
            }
            let _: Value = c.post(&format!("/users/{}/password", u.id), &json!({ "password": p1 })).await?;
            println!("Password reset for {}. Endpoints pick it up on their next check-in.", u.username);
        }
        UserCmd::Delete { username } => {
            let u = find_user(&c, &username).await?;
            c.delete(&format!("/users/{}", u.id)).await?;
            println!("Deleted {}.", u.username);
        }
    }
    Ok(())
}

async fn group(cmd: GroupCmd, json_out: bool) -> Result<()> {
    let c = client()?;
    match cmd {
        GroupCmd::List => {
            let gs: Vec<Group> = c.get("/groups").await?;
            if json_out {
                return print_json(&gs);
            }
            table(&["name", "members", "description"], gs.iter().map(|g| vec![g.name.clone(), g.member_ids.len().to_string(), g.description.clone()]).collect());
        }
        GroupCmd::Add { name, description } => {
            let g: Group = c.post("/groups", &json!({ "name": name, "description": description })).await?;
            println!("Created group {}.", g.name);
        }
        GroupCmd::Members { group } => {
            let g = find_group(&c, &group).await?;
            let users: Vec<User> = c.get("/users").await?;
            let members: Vec<&User> = users.iter().filter(|u| g.member_ids.contains(&u.id)).collect();
            if json_out {
                return print_json(&members);
            }
            table(&["username", "name"], members.iter().map(|u| vec![u.username.clone(), u.display_name.clone()]).collect());
        }
        GroupCmd::AddMember { group, username } => {
            let g = find_group(&c, &group).await?;
            let u = find_user(&c, &username).await?;
            let mut ids = g.member_ids.clone();
            if !ids.contains(&u.id) {
                ids.push(u.id);
            }
            let _: Group = c.put(&format!("/groups/{}/members", g.id), &json!({ "ids": ids })).await?;
            println!("Added {} to {}.", u.username, g.name);
        }
        GroupCmd::RemoveMember { group, username } => {
            let g = find_group(&c, &group).await?;
            let u = find_user(&c, &username).await?;
            let ids: Vec<_> = g.member_ids.iter().filter(|i| **i != u.id).collect();
            let _: Group = c.put(&format!("/groups/{}/members", g.id), &json!({ "ids": ids })).await?;
            println!("Removed {} from {}.", u.username, g.name);
        }
    }
    Ok(())
}

async fn endpoint(cmd: EndpointCmd, json_out: bool) -> Result<()> {
    let c = client()?;
    match cmd {
        EndpointCmd::List => {
            let eps: Vec<Endpoint> = c.get("/endpoints").await?;
            if json_out {
                return print_json(&eps);
            }
            table(
                &["hostname", "os", "version", "serial", "status", "last seen"],
                eps.iter()
                    .map(|e| {
                        vec![
                            e.hostname.clone(),
                            e.os.label().into(),
                            e.inventory.os_version.clone(),
                            e.inventory.serial_number.clone(),
                            if e.online { "Online".into() } else { "Offline".into() },
                            ago(e.last_seen_at),
                        ]
                    })
                    .collect(),
            );
        }
        EndpointCmd::Show { hostname } => {
            let e = find_endpoint(&c, &hostname).await?;
            if json_out {
                return print_json(&e);
            }
            let i = &e.inventory;
            println!("{} ({})", e.hostname, if e.online { "online" } else { "offline" });
            println!("  OS:          {} {} {}", i.os_name, i.os_version, i.os_edition);
            println!("  Hardware:    {} {}", i.manufacturer, i.model);
            println!("  Serial:      {}", i.serial_number);
            println!("  CPU:         {} ({} cores)", i.cpu_model, i.cpu_cores);
            println!("  Memory:      {:.1} GB", i.memory_bytes as f64 / 1e9);
            for n in &i.interfaces {
                println!("  {:<12} {} {}", format!("{}:", n.name), n.mac, n.ips.join(", "));
            }
            println!("  Users:       {}", i.logged_in_users.join(", "));
            println!("  Agent:       {} · last seen {}", e.agent_version, ago(e.last_seen_at));
            println!("  Compliance:");
            for c in &e.compliance {
                println!("    {:<20} {:<14} {}", c.setting, format!("{:?}", c.state), c.message);
            }
        }
        EndpointCmd::Run { hostname, shell, script, no_wait } => {
            let e = find_endpoint(&c, &hostname).await?;
            let job: Job = c.post(&format!("/endpoints/{}/jobs", e.id), &json!({ "type": "script", "shell": shell, "script": script })).await?;
            if no_wait {
                println!("Queued job {}", job.id);
                return Ok(());
            }
            eprintln!("Queued on {}; waiting for the agent…", e.hostname);
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let j: Job = c.get(&format!("/jobs/{}", job.id)).await?;
                if matches!(j.status, JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled) {
                    print!("{}", j.output);
                    if !j.output.ends_with('\n') {
                        println!();
                    }
                    if j.status != JobStatus::Succeeded {
                        bail!("job {:?} (exit code {:?})", j.status, j.exit_code);
                    }
                    break;
                }
            }
        }
        EndpointCmd::Rsop { hostname } => {
            let e = find_endpoint(&c, &hostname).await?;
            let r: Vec<Value> = c.get(&format!("/endpoints/{}/rsop", e.id)).await?;
            if json_out {
                return print_json(&r);
            }
            table(
                &["setting", "from GPO", "state", "overrides"],
                r.iter()
                    .map(|x| {
                        vec![
                            s(&x["description"]),
                            s(&x["policy_name"]),
                            x["compliance"]["state"].as_str().unwrap_or("pending").to_string(),
                            x["overridden"].as_array().map(|a| a.iter().map(s).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
                        ]
                    })
                    .collect(),
            );
        }
        EndpointCmd::Retire { hostname } => {
            let e = find_endpoint(&c, &hostname).await?;
            c.delete(&format!("/endpoints/{}", e.id)).await?;
            println!("Retired {}.", e.hostname);
        }
    }
    Ok(())
}
