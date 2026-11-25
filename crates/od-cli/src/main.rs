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
    /// Create a new domain on a freshly installed Management Server (headless
    /// onboarding). Asks for anything not given as a flag.
    Init {
        /// e.g. https://10.0.0.4:8443
        #[arg(long)]
        server: String,
        #[arg(long)]
        fingerprint: Option<String>,
        #[arg(long)]
        organization: Option<String>,
        /// DNS-style domain name, e.g. corp.contoso.com
        #[arg(long)]
        domain: Option<String>,
        /// First Domain Admin's username.
        #[arg(long)]
        admin: Option<String>,
        #[arg(long)]
        admin_name: Option<String>,
        /// custom, minimal, home, small_business, enterprise, education or high_security
        #[arg(long)]
        baseline: Option<String>,
        /// relaxed, balanced, strict or maximum
        #[arg(long)]
        security_level: Option<String>,
        /// Compliance packs, comma separated: hipaa, pci_dss, soc2, cmmc, gdpr, ferpa, cipa
        #[arg(long, value_delimiter = ',')]
        compliance: Vec<String>,
        /// everyone, pilot or review
        #[arg(long)]
        rollout: Option<String>,
        /// Show the plan and stop without creating anything.
        #[arg(long)]
        dry_run: bool,
        /// Don't ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Script Library: list, show and run saved scripts and templates.
    #[command(subcommand)]
    Script(ScriptCmd),
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
        /// Label shown in the Admin Center, e.g. "Lab laptops".
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
enum ScriptCmd {
    /// List scripts in the library.
    List,
    /// List built-in templates.
    Templates,
    /// Add a template to the library.
    AddTemplate { key: String },
    /// Show a script.
    Show { name: String },
    /// Run a script. Combine targets freely.
    Run {
        name: String,
        /// Endpoint hostnames.
        #[arg(long = "endpoint", short = 'e')]
        endpoints: Vec<String>,
        /// Security groups (their computer members).
        #[arg(long = "group", short = 'g')]
        groups: Vec<String>,
        /// OUs (including child OUs).
        #[arg(long = "ou")]
        ous: Vec<String>,
        /// Every endpoint.
        #[arg(long)]
        all: bool,
        /// Parameter values, NAME=value.
        #[arg(long = "param", short = 'p')]
        params: Vec<String>,
        /// Show which endpoints would run it.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum UserCmd {
    /// List every user with status and role.
    List,
    /// Show one user's details and group membership.
    Show { username: String },
    /// Create a user (prompts for the initial password).
    Add {
        /// Sign-in name, e.g. alex.morgan (max 20 characters).
        username: String,
        /// Full name, e.g. "Alex Morgan".
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        email: Option<String>,
        /// Organizational unit to place the user in (default: Staff, if it exists).
        #[arg(long)]
        ou: Option<String>,
        /// Make this user a Domain Admin.
        #[arg(long)]
        admin: bool,
        /// Add to these security groups (repeatable).
        #[arg(long = "group")]
        groups: Vec<String>,
    },
    /// Disable a user; their endpoint accounts are locked at the next check-in.
    Disable { username: String },
    /// Re-enable a disabled user.
    Enable { username: String },
    /// Reset a password (synced to endpoints on their next check-in).
    Passwd { username: String },
    /// Delete a user. Their endpoint accounts are disabled, never deleted.
    Delete { username: String },
}

#[derive(Subcommand)]
enum GroupCmd {
    /// List security groups with member counts.
    List,
    /// Create a security group.
    Add {
        name: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    /// List a group's members.
    Members { group: String },
    /// Add a user to a group.
    AddMember { group: String, username: String },
    /// Remove a user from a group.
    RemoveMember { group: String, username: String },
}

#[derive(Subcommand)]
enum EndpointCmd {
    /// List endpoints with OS, status and compliance.
    List,
    /// Show an endpoint's inventory and compliance.
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
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config"))
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
        Cmd::Init {
            server,
            fingerprint,
            organization,
            domain,
            admin,
            admin_name,
            baseline,
            security_level,
            compliance,
            rollout,
            dry_run,
            yes,
        } => {
            let target = ServerTarget { url: server.trim_end_matches('/').into(), fingerprint };
            let c = Client::new(&target)?;
            let st: Value = c.get("/setup").await?;
            if st["needs_setup"] == false && !dry_run {
                bail!("This Management Server already has a domain ({}). Use `od login` instead.", st["domain"].as_str().unwrap_or("?"));
            }
            let organization = match organization {
                Some(o) => o,
                None => ask("Organization name", None)?,
            };
            let suggested =
                format!("corp.{}.local", organization.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>());
            let domain = match domain {
                Some(d) => d,
                None => ask("Domain name", Some(&suggested))?,
            };
            let admin_name = match admin_name {
                Some(n) => n,
                None => ask("Your full name", None)?,
            };
            let first = admin_name.split_whitespace().next().unwrap_or("admin").to_lowercase();
            let admin = match admin {
                Some(a) => a,
                None => ask("Admin username", Some(&first))?,
            };
            let cat: Value = c.get("/baselines").await?;
            let baseline = match baseline {
                Some(b) => b,
                None => {
                    println!("\nSecurity baselines:");
                    for b in cat["baselines"].as_array().into_iter().flatten() {
                        println!(
                            "  {:<15} {}: {}",
                            b["id"].as_str().unwrap_or(""),
                            b["name"].as_str().unwrap_or(""),
                            b["tagline"].as_str().unwrap_or("")
                        );
                    }
                    ask("Baseline", Some("small_business"))?
                }
            };
            let mut modifiers = json!({ "compliance": compliance });
            if let Some(l) = security_level {
                modifiers["security_level"] = json!(l);
            }
            if let Some(r) = rollout {
                modifiers["rollout"] = json!(r);
            }
            let plan: Value = c.post("/baselines/preview", &json!({ "baseline": baseline, "modifiers": modifiers })).await?;
            if json_out && dry_run {
                return print_json(&plan);
            }
            println!("\nPlan for {organization} ({domain}), {baseline} baseline:");
            let names = |k: &str, f: &str| {
                plan[k]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x[f].as_str().map(str::to_string)).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            };
            println!("  OUs:     {}", names("ous", "name"));
            println!(
                "  Groups:  Domain Admins, Domain Users{}",
                plan["groups"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|g| g["name"].as_str()).map(|n| format!(", {n}")).collect::<String>())
                    .unwrap_or_default()
            );
            for p in plan["policies"].as_array().into_iter().flatten() {
                println!(
                    "  GPO:     {} ({} settings)",
                    p["name"].as_str().unwrap_or(""),
                    p["settings"].as_array().map(|a| a.len()).unwrap_or(0)
                );
            }
            for n in plan["notes"].as_array().into_iter().flatten() {
                println!("  note:    {}", n["text"].as_str().unwrap_or(""));
            }
            if dry_run {
                return Ok(());
            }
            let password = prompt_password(&format!("Password for {admin} (8+ characters): "))?;
            if std::env::var("OD_PASSWORD").is_err() && prompt_password("Confirm password: ")? != password {
                bail!("The passwords don't match");
            }
            if !yes && !ask("Create this domain? (yes/no)", Some("yes"))?.to_lowercase().starts_with('y') {
                bail!("Cancelled; nothing was created");
            }
            let resp: Value = c
                .post(
                    "/setup",
                    &json!({ "organization": organization, "domain": domain, "admin_username": admin, "admin_display_name": admin_name,
                             "admin_password": password, "baseline": baseline, "modifiers": modifiers }),
                )
                .await?;
            let token = resp["token"].as_str().context("no token in response")?.to_string();
            let name = resp["user"]["username"].as_str().unwrap_or(&admin).to_string();
            save_config(&Config { server: target, token, username: name.clone() })?;
            println!("\nDomain {domain} created. Signed in as {name}.");
            println!("Next: `od enroll` prints the command to enroll a computer.");
        }
        Cmd::Script(cmd) => {
            let c = client()?;
            run_script_cmd(&c, cmd, json_out).await?;
        }
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
                    println!(
                        "{}{}{}",
                        "  ".repeat(depth),
                        o.name,
                        if o.description.is_empty() { String::new() } else { format!("  ({})", o.description) }
                    );
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
            let t: EnrollmentToken =
                c.post("/enrollment-tokens", &json!({ "description": description, "ou_id": ou_id, "max_uses": uses })).await?;
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
                                    if p.targets.all_endpoints {
                                        "All endpoints".into()
                                    } else {
                                        format!("{} OU(s)", p.targets.ou_ids.len())
                                    },
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
                a.iter()
                    .map(|e| {
                        vec![
                            e.at.format("%Y-%m-%d %H:%M:%S").to_string(),
                            e.actor.clone(),
                            e.action.clone(),
                            e.target.clone(),
                            e.details.clone(),
                        ]
                    })
                    .collect(),
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
        UserCmd::Add { username, name, email, ou, admin, groups } => {
            let ou_id = match ou {
                Some(o) => Some(find_ou(&c, &o).await?.id),
                None => c.get::<Vec<OrgUnit>>("/ous").await?.into_iter().find(|o| o.name == "Staff" && o.parent_id.is_none()).map(|o| o.id),
            };
            let password = prompt_password(&format!("Initial password for {username}: "))?;
            let mut gids = vec![];
            for g in groups {
                gids.push(find_group(&c, &g).await?.id);
            }
            let u: User = c
                .post(
                    "/users",
                    &json!({ "username": username, "display_name": name, "email": email, "role": if admin { "admin" } else { "user" }, "password": password, "group_ids": gids, "ou_id": ou_id }),
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
            table(
                &["name", "members", "description"],
                gs.iter().map(|g| vec![g.name.clone(), g.member_ids.len().to_string(), g.description.clone()]).collect(),
            );
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
            let job: Job =
                c.post(&format!("/endpoints/{}/jobs", e.id), &json!({ "type": "script", "shell": shell, "script": script })).await?;
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

/// Ask on the terminal, with an optional default.
fn ask(label: &str, default: Option<&str>) -> Result<String> {
    use std::io::Write;
    loop {
        match default {
            Some(d) => print!("{label} [{d}]: "),
            None => print!("{label}: "),
        }
        std::io::stdout().flush()?;
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line)? == 0 {
            bail!("No input (use flags to run non-interactively; see `od init --help`)");
        }
        let v = line.trim();
        if !v.is_empty() {
            return Ok(v.to_string());
        }
        if let Some(d) = default {
            return Ok(d.to_string());
        }
    }
}

async fn find_script(c: &Client, name: &str) -> Result<od_core::scripts::Script> {
    let all: Vec<od_core::scripts::Script> = c.get("/scripts").await?;
    all.into_iter().find(|s| s.name.eq_ignore_ascii_case(name) || s.id.to_string() == name).ok_or_else(|| anyhow!("No script named {name}"))
}

async fn run_script_cmd(c: &Client, cmd: ScriptCmd, json_out: bool) -> Result<()> {
    match cmd {
        ScriptCmd::List => {
            let all: Vec<od_core::scripts::Script> = c.get("/scripts").await?;
            if json_out {
                return print_json(&all);
            }
            table(
                &["name", "category", "shell", "os", "params"],
                all.iter()
                    .map(|s| {
                        vec![
                            s.name.clone(),
                            s.category.clone(),
                            format!("{:?}", s.shell).to_lowercase(),
                            if s.os.is_empty() { "any".into() } else { s.os.iter().map(|o| o.label()).collect::<Vec<_>>().join(",") },
                            s.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(","),
                        ]
                    })
                    .collect(),
            );
        }
        ScriptCmd::Templates => {
            let t: Value = c.get("/scripts/templates").await?;
            if json_out {
                return print_json(&t);
            }
            table(
                &["key", "name", "os"],
                t.as_array()
                    .into_iter()
                    .flatten()
                    .map(|x| {
                        vec![
                            x["key"].as_str().unwrap_or("").into(),
                            x["name"].as_str().unwrap_or("").into(),
                            x["os"]
                                .as_array()
                                .map(|a| a.iter().filter_map(|o| o.as_str()).collect::<Vec<_>>().join(","))
                                .unwrap_or_default(),
                        ]
                    })
                    .collect(),
            );
        }
        ScriptCmd::AddTemplate { key } => {
            let s: od_core::scripts::Script = c.post("/scripts", &json!({ "template": key })).await?;
            println!("Added \"{}\" to the library.", s.name);
        }
        ScriptCmd::Show { name } => {
            let s = find_script(c, &name).await?;
            if json_out {
                return print_json(&s);
            }
            println!("{}  ({}, {:?})\n{}\n", s.name, s.category, s.shell, s.description);
            for p in &s.params {
                println!("  param {} = {:?}{}  {}", p.name, p.default, if p.required { " (required)" } else { "" }, p.label);
            }
            println!("\n{}", s.body);
        }
        ScriptCmd::Run { name, endpoints, groups, ous, all, params, dry_run } => {
            let s = find_script(c, &name).await?;
            let mut eids = vec![];
            for e in &endpoints {
                eids.push(find_endpoint(c, e).await?.id);
            }
            let mut gids = vec![];
            for g in &groups {
                gids.push(find_group(c, g).await?.id);
            }
            let mut oids = vec![];
            for o in &ous {
                oids.push(find_ou(c, o).await?.id);
            }
            let mut pv = serde_json::Map::new();
            for kv in &params {
                let (k, v) = kv.split_once('=').ok_or_else(|| anyhow!("Parameters look like NAME=value"))?;
                pv.insert(k.trim().to_uppercase(), json!(v));
            }
            let body = json!({ "targets": { "all_endpoints": all, "endpoint_ids": eids, "group_ids": gids, "ou_ids": oids }, "params": pv, "dry_run": dry_run });
            let r: Value = c.post(&format!("/scripts/{}/run", s.id), &body).await?;
            if json_out {
                return print_json(&r);
            }
            let hosts = |k: &str| {
                r[k].as_array().map(|a| a.iter().filter_map(|t| t["hostname"].as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
            };
            if dry_run {
                println!("Would run on: {}", hosts("targets"));
            } else {
                println!("Queued on: {}", hosts("targets"));
                println!("See results with `od endpoint show <hostname>` or in the Admin Center's Script Library.");
            }
            if r["skipped"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                println!("Skipped (wrong OS): {}", hosts("skipped"));
            }
        }
    }
    Ok(())
}
