//! Local sign-in accounts for directory users.
//!
//! The agent creates real local accounts so people sign in at the normal
//! login screen with their OpenDirectory username and password. This is how
//! machines that can't join an AD domain (Windows Home, macOS, Linux) get
//! centrally managed sign-in. Safety rules:
//!   * only accounts the agent created itself are ever modified;
//!   * accounts are disabled, never deleted, when access is removed;
//!   * system account names are refused;
//!   * passwords travel over stdin, never on a command line.

use std::collections::{BTreeMap, HashSet};

use anyhow::{bail, Result};
use od_core::protocol::ManagedAccount;
use od_core::{ComplianceItem, ComplianceState};

use crate::state::ManagedAccountState;

const RESERVED: &[&str] = &[
    "root",
    "administrator",
    "admin",
    "guest",
    "system",
    "daemon",
    "bin",
    "sys",
    "nobody",
    "defaultaccount",
    "wdagutilityaccount",
    "krbtgt",
    "wheel",
    "sudo",
    "staff",
    "everyone",
    "users",
    "administrators",
];

pub fn safe_name(name: &str) -> bool {
    let ok = !name.is_empty()
        && name.len() <= 20
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-' || c == '_');
    ok && !RESERVED.contains(&name)
}

/// Bring local accounts in line with what the server wants. Returns one
/// compliance item summarising the result.
pub async fn sync(desired: &[ManagedAccount], managed: &mut BTreeMap<String, ManagedAccountState>) -> Option<ComplianceItem> {
    if desired.is_empty() && managed.values().all(|m| !m.enabled) {
        return None;
    }
    let mut errors = Vec::new();
    let mut changes = 0;
    let wanted: HashSet<&str> = desired.iter().map(|a| a.username.as_str()).collect();

    for acct in desired {
        let name = acct.username.as_str();
        if !safe_name(name) {
            errors.push(format!("{name}: refused, not a safe account name"));
            continue;
        }
        match apply_one(acct, managed.get(name).cloned()).await {
            Ok(Some(new_state)) => {
                if managed.get(name) != Some(&new_state) {
                    changes += 1;
                }
                managed.insert(name.to_string(), new_state);
            }
            Ok(None) => {}
            Err(e) => errors.push(format!("{name}: {e:#}")),
        }
    }

    // Access removed: disable (never delete) accounts we manage.
    let stale: Vec<String> = managed.iter().filter(|(n, s)| s.enabled && !wanted.contains(n.as_str())).map(|(n, _)| n.clone()).collect();
    for name in stale {
        let res = async {
            os::set_admin(&name, false).await?;
            os::set_enabled(&name, false).await
        }
        .await;
        match res {
            Ok(()) => {
                if let Some(s) = managed.get_mut(&name) {
                    s.enabled = false;
                    s.admin = false;
                }
                changes += 1;
            }
            Err(e) => errors.push(format!("{name}: couldn't disable: {e:#}")),
        }
    }

    let active = managed.values().filter(|m| m.enabled).count();
    Some(if errors.is_empty() {
        ComplianceItem {
            setting: "local_accounts".into(),
            state: if changes > 0 { ComplianceState::Applied } else { ComplianceState::Compliant },
            message: format!("{active} directory account(s) can sign in"),
        }
    } else {
        ComplianceItem { setting: "local_accounts".into(), state: ComplianceState::Failed, message: errors.join("; ") }
    })
}

async fn apply_one(acct: &ManagedAccount, current: Option<ManagedAccountState>) -> Result<Option<ManagedAccountState>> {
    let name = acct.username.as_str();
    let mut st = match current {
        Some(s) => s,
        None => {
            if os::exists(name).await? {
                bail!("a local account with this name already exists and isn't managed by OpenDirectory");
            }
            let Some(pw) = &acct.password else {
                bail!("waiting for the password from the Management Server");
            };
            os::create(name, &acct.display_name, pw).await?;
            ManagedAccountState {
                password_version: acct.password_version,
                admin: false,
                enabled: true,
                display_name: acct.display_name.clone(),
            }
        }
    };
    if !os::exists(name).await? {
        // Someone deleted it locally; recreate on the next round once we have the password.
        let Some(pw) = &acct.password else {
            bail!("account was removed locally; it will be recreated when the password is next synced");
        };
        os::create(name, &acct.display_name, pw).await?;
        st.enabled = true;
        st.admin = false;
    }
    if let Some(pw) = &acct.password {
        if st.password_version != acct.password_version {
            os::set_password(name, pw).await?;
        }
    }
    st.password_version = acct.password_version;
    if st.display_name != acct.display_name {
        os::set_display_name(name, &acct.display_name).await?;
        st.display_name = acct.display_name.clone();
    }
    // A disabled account never keeps admin rights.
    let want_admin = acct.admin && acct.enabled;
    if st.admin != want_admin {
        os::set_admin(name, want_admin).await?;
        st.admin = want_admin;
    }
    if st.enabled != acct.enabled {
        os::set_enabled(name, acct.enabled).await?;
        st.enabled = acct.enabled;
    }
    Ok(Some(st))
}

fn check(o: crate::util::Output, what: &str) -> Result<()> {
    if o.ok() {
        Ok(())
    } else {
        bail!("{what} failed: {}", o.message())
    }
}

#[cfg(target_os = "linux")]
pub(crate) mod os {
    use super::check;
    use crate::util::{run, run_stdin};
    use anyhow::Result;

    fn admin_group() -> &'static str {
        if std::fs::read_to_string("/etc/group").map(|g| g.lines().any(|l| l.starts_with("sudo:"))).unwrap_or(false) {
            "sudo"
        } else {
            "wheel"
        }
    }

    pub async fn exists(name: &str) -> Result<bool> {
        Ok(run("getent", &["passwd", name]).await?.ok())
    }
    pub async fn create(name: &str, display: &str, password: &str) -> Result<()> {
        check(run("useradd", &["--create-home", "--shell", "/bin/bash", "--comment", display, name]).await?, "useradd")?;
        set_password(name, password).await
    }
    pub async fn set_password(name: &str, password: &str) -> Result<()> {
        check(run_stdin("chpasswd", &[], &format!("{name}:{password}\n")).await?, "chpasswd")
    }
    pub async fn set_display_name(name: &str, display: &str) -> Result<()> {
        check(run("usermod", &["--comment", display, name]).await?, "usermod")
    }
    pub async fn set_enabled(name: &str, enabled: bool) -> Result<()> {
        if enabled {
            check(run("usermod", &["--unlock", "--expiredate", "", name]).await?, "usermod --unlock")
        } else {
            check(run("usermod", &["--lock", "--expiredate", "1", name]).await?, "usermod --lock")
        }
    }
    pub async fn set_admin(name: &str, admin: bool) -> Result<()> {
        let g = admin_group();
        if admin {
            check(run("usermod", &["--append", "--groups", g, name]).await?, "usermod -aG")
        } else {
            let o = run("gpasswd", &["--delete", name, g]).await?;
            // Not being a member already is fine.
            if o.ok() || o.stderr.contains("not a member") {
                Ok(())
            } else {
                check(o, "gpasswd -d")
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod os {
    use super::check;
    use crate::util::{run, run_stdin};
    use anyhow::{bail, Result};

    pub async fn exists(name: &str) -> Result<bool> {
        Ok(run("dscl", &[".", "-read", &format!("/Users/{name}"), "UniqueID"]).await?.ok())
    }

    async fn next_uid() -> Result<u32> {
        let o = run("dscl", &[".", "-list", "/Users", "UniqueID"]).await?;
        let max = o
            .stdout
            .lines()
            .filter_map(|l| l.split_whitespace().nth(1)?.parse::<u32>().ok())
            .filter(|u| *u >= 501 && *u < 60000)
            .max()
            .unwrap_or(500);
        Ok(max + 1)
    }

    pub async fn create(name: &str, display: &str, password: &str) -> Result<()> {
        let uid = next_uid().await?;
        let p = format!("/Users/{name}");
        // dscl in interactive mode reads commands from stdin, keeping the password off the command line.
        let script = format!(
            "create {p}\ncreate {p} UserShell /bin/zsh\ncreate {p} RealName \"{}\"\ncreate {p} UniqueID {uid}\ncreate {p} PrimaryGroupID 20\ncreate {p} NFSHomeDirectory {p}\npasswd {p} \"{}\"\nquit\n",
            display.replace('"', "'"),
            password.replace('\\', "\\\\").replace('"', "\\\"")
        );
        check(run_stdin("dscl", &["."], &script).await?, "dscl create")?;
        let _ = run("createhomedir", &["-c", "-u", name]).await;
        if !exists(name).await? {
            bail!("account was not created");
        }
        Ok(())
    }
    pub async fn set_password(name: &str, password: &str) -> Result<()> {
        let script = format!("passwd /Users/{name} \"{}\"\nquit\n", password.replace('\\', "\\\\").replace('"', "\\\""));
        check(run_stdin("dscl", &["."], &script).await?, "dscl passwd")
    }
    pub async fn set_display_name(name: &str, display: &str) -> Result<()> {
        check(run("dscl", &[".", "-create", &format!("/Users/{name}"), "RealName", display]).await?, "dscl RealName")
    }
    pub async fn set_enabled(name: &str, enabled: bool) -> Result<()> {
        let flag = if enabled { "-enableuser" } else { "-disableuser" };
        check(run("pwpolicy", &["-u", name, flag]).await?, "pwpolicy")
    }
    pub async fn set_admin(name: &str, admin: bool) -> Result<()> {
        let op = if admin { "-a" } else { "-d" };
        let o = run("dseditgroup", &["-o", "edit", op, name, "-t", "user", "admin"]).await?;
        if o.ok() || !admin {
            Ok(())
        } else {
            check(o, "dseditgroup")
        }
    }
}

#[cfg(target_os = "windows")]
pub(crate) mod os {
    use super::check;
    use crate::util::powershell;
    use anyhow::Result;

    // Group SIDs avoid localized names ("Administratoren", "Utilisateurs", ...).
    const ADMINS: &str = "S-1-5-32-544";
    const USERS: &str = "S-1-5-32-545";

    pub async fn exists(name: &str) -> Result<bool> {
        let o = powershell(
            "$n=[Console]::In.ReadLine(); if (Get-LocalUser -Name $n -ErrorAction SilentlyContinue) { 'yes' } else { 'no' }",
            Some(&format!("{name}\n")),
        )
        .await?;
        Ok(o.stdout.trim() == "yes")
    }
    pub async fn create(name: &str, display: &str, password: &str) -> Result<()> {
        let script = format!(
            "$ErrorActionPreference='Stop'; $n=[Console]::In.ReadLine(); $f=[Console]::In.ReadLine(); $p=ConvertTo-SecureString ([Console]::In.ReadLine()) -AsPlainText -Force; \
             New-LocalUser -Name $n -FullName $f -Password $p -PasswordNeverExpires -AccountNeverExpires | Out-Null; \
             Add-LocalGroupMember -SID '{USERS}' -Member $n -ErrorAction SilentlyContinue"
        );
        check(powershell(&script, Some(&format!("{name}\n{display}\n{password}\n"))).await?, "New-LocalUser")
    }
    pub async fn set_password(name: &str, password: &str) -> Result<()> {
        let script = "$ErrorActionPreference='Stop'; $n=[Console]::In.ReadLine(); $p=ConvertTo-SecureString ([Console]::In.ReadLine()) -AsPlainText -Force; Set-LocalUser -Name $n -Password $p";
        check(powershell(script, Some(&format!("{name}\n{password}\n"))).await?, "Set-LocalUser")
    }
    pub async fn set_display_name(name: &str, display: &str) -> Result<()> {
        let script =
            "$ErrorActionPreference='Stop'; $n=[Console]::In.ReadLine(); $f=[Console]::In.ReadLine(); Set-LocalUser -Name $n -FullName $f";
        check(powershell(script, Some(&format!("{name}\n{display}\n"))).await?, "Set-LocalUser")
    }
    pub async fn set_enabled(name: &str, enabled: bool) -> Result<()> {
        let cmd = if enabled { "Enable-LocalUser" } else { "Disable-LocalUser" };
        let script = format!("$ErrorActionPreference='Stop'; $n=[Console]::In.ReadLine(); {cmd} -Name $n");
        check(powershell(&script, Some(&format!("{name}\n"))).await?, cmd)
    }
    pub async fn set_admin(name: &str, admin: bool) -> Result<()> {
        let script = if admin {
            format!("$n=[Console]::In.ReadLine(); Add-LocalGroupMember -SID '{ADMINS}' -Member $n -ErrorAction SilentlyContinue; exit 0")
        } else {
            format!("$n=[Console]::In.ReadLine(); Remove-LocalGroupMember -SID '{ADMINS}' -Member $n -ErrorAction SilentlyContinue; exit 0")
        };
        check(powershell(&script, Some(&format!("{name}\n"))).await?, "local Administrators membership")
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(crate) mod os {
    use anyhow::{bail, Result};
    pub async fn exists(_: &str) -> Result<bool> {
        bail!("unsupported OS")
    }
    pub async fn create(_: &str, _: &str, _: &str) -> Result<()> {
        bail!("unsupported OS")
    }
    pub async fn set_password(_: &str, _: &str) -> Result<()> {
        bail!("unsupported OS")
    }
    pub async fn set_display_name(_: &str, _: &str) -> Result<()> {
        bail!("unsupported OS")
    }
    pub async fn set_enabled(_: &str, _: bool) -> Result<()> {
        bail!("unsupported OS")
    }
    pub async fn set_admin(_: &str, _: bool) -> Result<()> {
        bail!("unsupported OS")
    }
}

#[cfg(test)]
mod tests {
    use super::safe_name;
    #[test]
    fn refuses_system_accounts() {
        assert!(safe_name("alice"));
        assert!(safe_name("j.doe-2"));
        assert!(!safe_name("root"));
        assert!(!safe_name("Administrator"));
        assert!(!safe_name("1abc"));
        assert!(!safe_name("a b"));
        assert!(!safe_name("averyveryverylongusername"));
    }
}
