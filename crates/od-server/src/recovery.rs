//! Break-glass recovery for the Management Server console.
//!
//! When every Domain Admin is locked out of the Admin Center, a local
//! administrator on the Management Server host can list the Domain Admins
//! and reset one's password with `opendirectory-server admin ...`. These
//! commands open the directory database on disk directly. There is no API
//! route for them, so they can't be reached over the network, and they
//! refuse to run without root (Administrator on Windows) or the account
//! that owns the data directory. Every reset is written to the audit log.

use std::io::{BufRead, IsTerminal};
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use od_core::Role;

use crate::crypto::Sealer;
use crate::db::Db;

/// Where the Admin Center's built-in Management Server keeps its data, per OS.
pub const ADMIN_CENTER_DATA_HINT: &str = if cfg!(target_os = "windows") {
    "%APPDATA%\\dev.opendirectory.admincenter\\server"
} else if cfg!(target_os = "macos") {
    "~/Library/Application Support/dev.opendirectory.admincenter/server"
} else {
    "~/.local/share/dev.opendirectory.admincenter/server"
};

/// How the new password is chosen.
pub enum NewPassword {
    /// Ask on the terminal twice, without echo.
    Prompt,
    /// Read one line from standard input (for scripts).
    Stdin,
    /// Generate a random password and print it once.
    Generate,
    /// Already chosen by the caller (used by tests).
    Given(String),
}

pub struct ResetOptions {
    pub username: String,
    pub password: NewPassword,
    /// Also re-enable the account if it was disabled.
    pub enable: bool,
    /// What started the reset, for the audit log (e.g. the Admin Center's
    /// "Forgot password?"). `None` means the command line.
    pub via: Option<String>,
}

/// The local OS account running the command, as written to the audit log.
fn operator() -> String {
    let user = std::env::var("SUDO_USER")
        .or_else(|_| pkexec_user().ok_or(std::env::VarError::NotPresent))
        .or_else(|_| std::env::var("USER"))
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".into());
    let host = crate::tls::hostname_guess();
    if host.is_empty() {
        format!("local:{user}")
    } else {
        format!("local:{user}@{host}")
    }
}

/// True when this process runs as root, or elevated as Administrator on
/// Windows. Used by callers that must not accept "owns the data directory".
pub fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions and can't fail.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        // SAFETY: IsUserAnAdmin takes no arguments.
        unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
    }
}

/// The Domain Admins' logon and display names, for an account picker.
pub fn admin_accounts(data_dir: &Path) -> Result<Vec<(String, String, bool)>> {
    let db_path = data_dir.join("directory.db");
    if !db_path.exists() {
        bail!("No directory found in {}", data_dir.display());
    }
    let db = Db::open(&db_path)?;
    Ok(db.list_users()?.into_iter().filter(|u| u.role == Role::Admin).map(|u| (u.username, u.display_name, u.enabled)).collect())
}

/// The desktop user who ran `pkexec`, looked up from `PKEXEC_UID`.
fn pkexec_user() -> Option<String> {
    let uid = std::env::var("PKEXEC_UID").ok()?;
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() > 2 && f[2] == uid).then(|| f[0].to_string())
    })
}

/// Refuse unless this process has local administrator rights over `data_dir`.
fn require_local_admin(data_dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no preconditions and can't fail.
        let euid = unsafe { libc::geteuid() };
        let owner = std::fs::metadata(data_dir).map(|m| m.uid()).ok();
        if euid != 0 && owner != Some(euid) {
            bail!(
                "Break-glass recovery needs root on the Management Server host. Run it again with sudo, \
                 or as the account that owns {}.",
                data_dir.display()
            );
        }
    }
    #[cfg(windows)]
    {
        // SAFETY: IsUserAnAdmin takes no arguments; it's true only in an elevated process.
        if unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() } == 0 {
            bail!("Break-glass recovery needs Administrator rights. Open an elevated PowerShell (Run as administrator) and try again.");
        }
    }
    Ok(())
}

/// Open the directory of an existing Management Server, never creating one.
fn open(data_dir: &Path) -> Result<(Db, Sealer)> {
    let db_path = data_dir.join("directory.db");
    let key_path = data_dir.join("master.key");
    if !db_path.exists() || !key_path.exists() {
        bail!(
            "No directory found in {}. Pass the Management Server's data directory with --data-dir. \
             The Admin Center's built-in server keeps it in {ADMIN_CENTER_DATA_HINT}.",
            data_dir.display()
        );
    }
    require_local_admin(data_dir)?;
    let db = Db::open(&db_path)?;
    // The server may be running and holding a write lock for a moment.
    db.lock().busy_timeout(std::time::Duration::from_secs(10))?;
    let sealer = Sealer::load_or_create(&key_path)?;
    Ok((db, sealer))
}

/// Accept `DOMAIN\user` and `user@domain`, like the sign-in screen does.
fn normalize(name: &str) -> String {
    let n = name.trim().to_lowercase();
    let n = n.rsplit('\\').next().unwrap_or(&n).to_string();
    n.split('@').next().unwrap_or(&n).to_string()
}

/// `opendirectory-server admin list`: the Domain Admins, so you know which
/// account to recover.
pub fn list_admins(data_dir: &Path) -> Result<()> {
    let (db, _) = open(data_dir)?;
    let domain = db.meta_get("domain")?.unwrap_or_default();
    let admins: Vec<_> = db.list_users()?.into_iter().filter(|u| u.role == Role::Admin).collect();
    if admins.is_empty() {
        println!("This directory has no Domain Admins yet. Open the Admin Center to run the setup wizard.");
        return Ok(());
    }
    println!("Domain Admins in {domain}:");
    for u in admins {
        let state = if u.enabled { "enabled" } else { "disabled" };
        let last = u.last_login_at.map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string()).unwrap_or_else(|| "never".into());
        println!("  {:<20} {:<24} {:<8} last sign-in {last}", u.username, u.display_name, state);
    }
    Ok(())
}

/// A random password that meets any complexity rule: 20 characters drawn
/// from four character classes, with at least one of each.
fn generate_password() -> String {
    use rand::seq::SliceRandom;
    use rand::Rng;
    const SETS: [&[u8]; 4] = [b"ABCDEFGHJKLMNPQRSTUVWXYZ", b"abcdefghijkmnopqrstuvwxyz", b"23456789", b"-_.!@#%+="];
    let mut rng = rand::thread_rng();
    let mut out: Vec<u8> = SETS.iter().map(|s| s[rng.gen_range(0..s.len())]).collect();
    let all: Vec<u8> = SETS.concat();
    while out.len() < 20 {
        out.push(all[rng.gen_range(0..all.len())]);
    }
    out.shuffle(&mut rng);
    String::from_utf8(out).expect("ASCII")
}

fn read_password(how: &NewPassword) -> Result<String> {
    match how {
        NewPassword::Generate => Ok(generate_password()),
        NewPassword::Given(p) => Ok(p.clone()),
        NewPassword::Stdin => {
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            Ok(line.trim_end_matches(['\r', '\n']).to_string())
        }
        NewPassword::Prompt => {
            if !std::io::stdin().is_terminal() {
                bail!("No terminal to ask for the password on. Use --password-stdin or --generate.");
            }
            let a = rpassword::prompt_password("New password: ")?;
            let b = rpassword::prompt_password("Confirm new password: ")?;
            if a != b {
                bail!("The passwords don't match. Nothing was changed.");
            }
            Ok(a)
        }
    }
}

/// `opendirectory-server admin reset-password <user>`.
pub fn reset_password(data_dir: &Path, opts: &ResetOptions) -> Result<()> {
    let (db, sealer) = open(data_dir)?;
    if crate::api::replication::Runtime::load(data_dir).is_replica() {
        bail!("This server is a replica and copies its directory from the primary. Run this on the primary Management Server.");
    }
    let name = normalize(&opts.username);
    let Some(mut user) = db.get_user_by_username(&name)? else {
        bail!("There's no account named \"{name}\". Run `opendirectory-server admin list` to see the Domain Admins.");
    };
    if user.role != Role::Admin {
        bail!(
            "\"{name}\" isn't a Domain Admin. Break-glass recovery only resets Domain Admins; \
             reset other users from the Admin Center once you're signed in."
        );
    }

    let password = read_password(&opts.password)?;
    crate::api::passwords::check_db(&db, Some(user.id), &password).map_err(|e| anyhow!("{}", e.message))?;
    crate::api::passwords::set_db(&db, &sealer, user.id, &password).map_err(|e| anyhow!("{}", e.message))?;
    let re_enabled = opts.enable && !user.enabled;
    if re_enabled {
        user.enabled = true;
        db.update_user(&user)?;
    }
    // Anyone signed in with the old password is signed out.
    db.delete_user_sessions(user.id)?;

    let actor = operator();
    let mut details = match &opts.via {
        Some(via) => format!("Break-glass reset from {via} after OS administrator approval; existing sessions signed out"),
        None => "Break-glass reset on the Management Server console; existing sessions signed out".to_string(),
    };
    if re_enabled {
        details.push_str("; account re-enabled");
    }
    db.audit(&actor, "user.password_reset.break_glass", &user.username, &details);

    let domain = db.meta_get("domain")?.unwrap_or_default();
    let netbios = domain.split('.').next().unwrap_or_default().to_uppercase();
    println!("Password reset for {} ({}). The change is in the audit log.", user.username, user.display_name);
    if matches!(opts.password, NewPassword::Generate) {
        println!();
        println!("  Temporary password: {password}");
        println!();
        println!("It's shown only once. Sign in, then choose your own with \"Change password\".");
    }
    if netbios.is_empty() {
        println!("Sign in to the Admin Center as {}.", user.username);
    } else {
        println!("Sign in to the Admin Center as {} or {netbios}\\{}.", user.username, user.username);
    }
    if !user.enabled {
        println!("Note: this account is disabled. Run the command again with --enable to turn it back on.");
    }
    println!("If the sign-in screen says there were too many failed attempts, wait for the lockout to end or restart the server.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_passwords_meet_complexity() {
        for _ in 0..50 {
            let p = generate_password();
            assert_eq!(p.len(), 20);
            assert!(p.chars().any(|c| c.is_ascii_uppercase()));
            assert!(p.chars().any(|c| c.is_ascii_lowercase()));
            assert!(p.chars().any(|c| c.is_ascii_digit()));
            assert!(p.chars().any(|c| !c.is_alphanumeric()));
        }
    }

    #[test]
    fn accepts_domain_forms() {
        assert_eq!(normalize("CORP\\Controller"), "controller");
        assert_eq!(normalize("controller@corp.test.local"), "controller");
        assert_eq!(normalize(" controller "), "controller");
    }
}
