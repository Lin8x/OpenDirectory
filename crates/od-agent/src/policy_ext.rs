//! Account, device, network and hardening settings (added in v0.3), applied
//! with each OS's native mechanism. Kept apart from `policy.rs` so each OS
//! section stays readable.

use od_core::policy::Setting;
use od_core::{ComplianceItem, ComplianceState};

#[allow(unused_imports)]
use crate::util::{ensure_file, run, which};

fn item(s: &Setting, state: ComplianceState, message: impl Into<String>) -> ComplianceItem {
    ComplianceItem { setting: s.kind().to_string(), state, message: message.into() }
}

#[allow(dead_code)]
fn ok_or_failed(s: &Setting, ok: bool, good: impl Into<String>, err: impl Into<String>) -> ComplianceItem {
    if ok {
        item(s, ComplianceState::Compliant, good)
    } else {
        item(s, ComplianceState::Failed, err)
    }
}

/// Set `key<sep>value` in a simple config file, replacing an existing (or
/// commented-out) line for the key, or appending one. Returns true if changed.
#[allow(dead_code)]
pub fn set_conf_key(path: &std::path::Path, key: &str, sep: &str, value: &str) -> anyhow::Result<bool> {
    let cur = std::fs::read_to_string(path).unwrap_or_default();
    let want = format!("{key}{sep}{value}");
    let mut found = false;
    let mut lines: Vec<String> = Vec::new();
    for l in cur.lines() {
        let t = l.trim_start().trim_start_matches('#').trim_start();
        let is_key = t.strip_prefix(key).map(|r| r.starts_with([' ', '\t', '='])).unwrap_or(false);
        if is_key && !found {
            lines.push(want.clone());
            found = true;
        } else {
            lines.push(l.to_string());
        }
    }
    if !found {
        lines.push(want);
    }
    let mut next = lines.join("\n");
    next.push('\n');
    if next == cur {
        return Ok(false);
    }
    crate::util::write_file(path, &next)?;
    Ok(true)
}

/// Set a key inside an `[section]` of an INI-style file. Returns true if changed.
#[allow(dead_code)]
pub fn set_ini_key(path: &std::path::Path, section: &str, key: &str, value: &str) -> anyhow::Result<bool> {
    let cur = std::fs::read_to_string(path).unwrap_or_default();
    let header = format!("[{section}]");
    let want = format!("{key}={value}");
    let mut out: Vec<String> = Vec::new();
    let mut in_sec = false;
    let mut done = false;
    for l in cur.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            if in_sec && !done {
                out.push(want.clone());
                done = true;
            }
            in_sec = t.eq_ignore_ascii_case(&header);
        } else if in_sec && !done {
            let bare = t.trim_start_matches('#').trim();
            if bare.split('=').next().map(|k| k.trim().eq_ignore_ascii_case(key)).unwrap_or(false) {
                out.push(want.clone());
                done = true;
                continue;
            }
        }
        out.push(l.to_string());
    }
    if !done {
        if !in_sec {
            out.push(header);
        }
        out.push(want);
    }
    let mut next = out.join("\n");
    next.push('\n');
    if next == cur {
        return Ok(false);
    }
    crate::util::write_file(path, &next)?;
    Ok(true)
}

// ====================================================================== Linux
#[cfg(target_os = "linux")]
async fn unit_exists(unit: &str) -> bool {
    run("systemctl", &["cat", unit]).await.map(|o| o.ok()).unwrap_or(false)
}

#[cfg(target_os = "linux")]
async fn set_service(unit: &str, on: bool) -> anyhow::Result<bool> {
    let args: &[&str] = if on { &["enable", "--now", unit] } else { &["disable", "--now", unit] };
    Ok(run("systemctl", args).await?.ok())
}

#[cfg(target_os = "linux")]
fn pam_uses(module: &str) -> bool {
    ["/etc/pam.d/common-auth", "/etc/pam.d/system-auth", "/etc/pam.d/common-password", "/etc/pam.d/password-auth"]
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .any(|c| c.lines().any(|l| !l.trim_start().starts_with('#') && l.contains(module)))
}

#[cfg(target_os = "linux")]
async fn dconf_local(name: &str, body: &str) -> anyhow::Result<Option<bool>> {
    use std::path::Path;
    if !Path::new("/etc/dconf").exists() || !which("dconf") {
        return Ok(None);
    }
    let profile = Path::new("/etc/dconf/profile/user");
    if !profile.exists() {
        ensure_file(profile, "user-db:user\nsystem-db:local\n")?;
    }
    let changed = ensure_file(&Path::new("/etc/dconf/db/local.d").join(name), body)?;
    if changed {
        run("dconf", &["update"]).await?;
    }
    Ok(Some(changed))
}

#[cfg(target_os = "linux")]
pub async fn apply(s: &Setting) -> anyhow::Result<ComplianceItem> {
    use std::path::Path;
    use ComplianceState::*;
    let st = |changed: bool| if changed { Applied } else { Compliant };
    Ok(match s {
        Setting::AccountLockout { threshold, duration_minutes } => {
            let p = Path::new("/etc/security/faillock.conf");
            let a = set_conf_key(p, "deny", " = ", &threshold.to_string())?;
            let b = set_conf_key(p, "unlock_time", " = ", &(duration_minutes * 60).to_string())?;
            if !pam_uses("pam_faillock") {
                if which("authselect") {
                    let o = run("authselect", &["enable-feature", "with-faillock"]).await?;
                    if !o.ok() {
                        return Ok(item(s, NonCompliant, format!("faillock configured but not active: {}", o.message())));
                    }
                } else {
                    return Ok(item(s, NonCompliant, "faillock.conf set, but pam_faillock isn't in this system's PAM stack"));
                }
            }
            item(s, st(a || b), format!("Lock after {threshold} failures for {duration_minutes} min"))
        }
        Setting::PasswordMaxAge { days } => {
            let v = if *days == 0 { 99999 } else { *days };
            let c = set_conf_key(Path::new("/etc/login.defs"), "PASS_MAX_DAYS", "\t", &v.to_string())?;
            item(
                s,
                st(c),
                if *days == 0 { "Passwords don't expire".to_string() } else { format!("PASS_MAX_DAYS {days} (applies to new passwords)") },
            )
        }
        Setting::PasswordHistory { count } => {
            let c = set_conf_key(Path::new("/etc/security/pwhistory.conf"), "remember", " = ", &count.to_string())?;
            if !pam_uses("pam_pwhistory") {
                return Ok(item(s, NonCompliant, "pwhistory.conf set, but pam_pwhistory isn't in this system's PAM stack"));
            }
            item(s, st(c), format!("Remembers {count} passwords"))
        }
        Setting::PasswordComplexity { required } => {
            let p = Path::new("/etc/security/pwquality.conf.d/51-opendirectory-complexity.conf");
            if *required {
                item(s, st(ensure_file(p, "# Managed by OpenDirectory\nminclass = 3\n")?), "pam_pwquality minclass = 3")
            } else {
                let _ = std::fs::remove_file(p);
                item(s, Compliant, "Not enforced")
            }
        }
        Setting::UsbStorage { mode } => {
            let block = Path::new("/etc/modprobe.d/opendirectory-usb-storage.conf");
            let ro = Path::new("/etc/udev/rules.d/80-opendirectory-usb-readonly.rules");
            match mode.as_str() {
                "block" => {
                    let _ = std::fs::remove_file(ro);
                    let c = ensure_file(
                        block,
                        "# Managed by OpenDirectory\ninstall usb-storage /bin/false\nblacklist usb-storage\nblacklist uas\n",
                    )?;
                    let _ = run("modprobe", &["-r", "uas", "usb-storage"]).await;
                    item(s, st(c), "usb-storage driver blocked")
                }
                "read_only" => {
                    let _ = std::fs::remove_file(block);
                    let c = ensure_file(
                        ro,
                        "# Managed by OpenDirectory\nSUBSYSTEM==\"block\", ENV{ID_BUS}==\"usb\", ACTION==\"add|change\", RUN+=\"/sbin/blockdev --setro /dev/%k\"\n",
                    )?;
                    if c {
                        let _ = run("udevadm", &["control", "--reload-rules"]).await;
                    }
                    item(s, st(c), "USB drives attach read-only")
                }
                _ => {
                    let a = std::fs::remove_file(block).is_ok();
                    let b = std::fs::remove_file(ro).is_ok();
                    item(s, st(a || b), "USB storage allowed")
                }
            }
        }
        Setting::GuestAccount { enabled } => {
            if Path::new("/etc/lightdm").exists() {
                let c = ensure_file(
                    Path::new("/etc/lightdm/lightdm.conf.d/50-opendirectory-guest.conf"),
                    &format!("# Managed by OpenDirectory\n[Seat:*]\nallow-guest={enabled}\n"),
                )?;
                item(s, st(c), if *enabled { "LightDM guest session on" } else { "LightDM guest session off" })
            } else {
                item(s, Compliant, "No guest session on this system")
            }
        }
        Setting::RemoteDesktop { enabled } => {
            let mut found = vec![];
            for u in ["xrdp.service", "gnome-remote-desktop.service"] {
                if unit_exists(u).await {
                    found.push(u);
                }
            }
            if found.is_empty() {
                return Ok(item(s, if *enabled { NonCompliant } else { Compliant }, "No remote desktop server installed"));
            }
            let mut ok = true;
            for u in &found {
                ok &= set_service(u, *enabled).await?;
            }
            ok_or_failed(
                s,
                ok,
                format!("{} {}", found.join(", "), if *enabled { "on" } else { "off" }),
                "Couldn't change the remote desktop service",
            )
        }
        Setting::SshServer { enabled } => {
            let unit = if unit_exists("ssh.service").await { "ssh.service" } else { "sshd.service" };
            if !unit_exists(unit).await {
                return Ok(item(s, if *enabled { NonCompliant } else { Compliant }, "OpenSSH server isn't installed"));
            }
            let active = run("systemctl", &["is-active", unit]).await?.stdout.trim() == "active";
            if active == *enabled {
                return Ok(item(s, Compliant, format!("{unit} {}", if active { "running" } else { "stopped" })));
            }
            if !*enabled {
                let _ = set_service("ssh.socket", false).await;
            }
            ok_or_failed(
                s,
                set_service(unit, *enabled).await?,
                format!("{unit} turned {}", if *enabled { "on" } else { "off" }),
                "Couldn't change sshd",
            )
        }
        Setting::AutoLogin { allowed } => {
            if *allowed {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            let mut changed = false;
            for p in ["/etc/gdm3/custom.conf", "/etc/gdm/custom.conf"] {
                if Path::new(p).exists() {
                    changed |= set_ini_key(Path::new(p), "daemon", "AutomaticLoginEnable", "false")?;
                }
            }
            if Path::new("/etc/lightdm").exists() {
                changed |= ensure_file(
                    Path::new("/etc/lightdm/lightdm.conf.d/50-opendirectory-autologin.conf"),
                    "# Managed by OpenDirectory\n[Seat:*]\nautologin-user=\n",
                )?;
            }
            item(s, st(changed), "Automatic sign-in off")
        }
        Setting::TimeServer { server } => {
            if unit_exists("systemd-timesyncd.service").await {
                let c = ensure_file(
                    Path::new("/etc/systemd/timesyncd.conf.d/50-opendirectory.conf"),
                    &format!("# Managed by OpenDirectory\n[Time]\nNTP={server}\n"),
                )?;
                if c {
                    run("systemctl", &["restart", "systemd-timesyncd"]).await?;
                }
                let _ = run("timedatectl", &["set-ntp", "true"]).await;
                item(s, st(c), format!("systemd-timesyncd uses {server}"))
            } else if Path::new("/etc/chrony.conf").exists() || Path::new("/etc/chrony/chrony.conf").exists() {
                let dir = if Path::new("/etc/chrony/sources.d").exists() {
                    "/etc/chrony/sources.d/opendirectory.sources"
                } else {
                    "/etc/chrony.d/opendirectory.conf"
                };
                let c = ensure_file(Path::new(dir), &format!("# Managed by OpenDirectory\nserver {server} iburst\n"))?;
                if c {
                    let _ = run("systemctl", &["restart", "chronyd"]).await;
                    let _ = run("systemctl", &["restart", "chrony"]).await;
                }
                item(s, st(c), format!("chrony uses {server}"))
            } else {
                item(s, NonCompliant, "Neither systemd-timesyncd nor chrony is installed")
            }
        }
        Setting::HideLastUsername { enabled } => {
            if !Path::new("/etc/dconf").exists() || !which("dconf") {
                return Ok(item(s, Unsupported, "No GNOME/dconf desktop found"));
            }
            ensure_file(
                Path::new("/etc/dconf/profile/gdm"),
                "user-db:user\nsystem-db:gdm\nfile-db:/usr/share/gdm/greeter-dconf-defaults\n",
            )?;
            let c = ensure_file(
                Path::new("/etc/dconf/db/gdm.d/50-opendirectory"),
                &format!("# Managed by OpenDirectory\n[org/gnome/login-screen]\ndisable-user-list={enabled}\n"),
            )?;
            if c {
                run("dconf", &["update"]).await?;
            }
            item(s, st(c), if *enabled { "GDM user list hidden" } else { "GDM user list shown" })
        }
        Setting::DisableAutorun { enabled } => {
            match dconf_local(
                "50-opendirectory-autorun",
                &format!("# Managed by OpenDirectory\n[org/gnome/desktop/media-handling]\nautorun-never={enabled}\n"),
            )
            .await?
            {
                Some(c) => item(s, st(c), if *enabled { "GNOME autorun off" } else { "GNOME autorun default" }),
                None => item(s, Compliant, "No desktop auto-run on this system"),
            }
        }
        Setting::Smb1 { enabled } => {
            let p = Path::new("/etc/samba/smb.conf");
            if !p.exists() {
                return Ok(item(s, Compliant, "Samba isn't installed"));
            }
            let c = set_ini_key(p, "global", "server min protocol", if *enabled { "NT1" } else { "SMB2" })?;
            if c {
                let _ = run("systemctl", &["reload", "smbd"]).await;
            }
            item(s, st(c), if *enabled { "Samba allows SMBv1" } else { "Samba requires SMB2 or newer" })
        }
        Setting::Llmnr { enabled } => {
            if !unit_exists("systemd-resolved.service").await {
                return Ok(item(s, Compliant, "systemd-resolved isn't used; no LLMNR responder"));
            }
            let c = ensure_file(
                Path::new("/etc/systemd/resolved.conf.d/50-opendirectory.conf"),
                &format!("# Managed by OpenDirectory\n[Resolve]\nLLMNR={}\n", if *enabled { "yes" } else { "no" }),
            )?;
            if c {
                let _ = run("systemctl", &["restart", "systemd-resolved"]).await;
            }
            item(s, st(c), format!("LLMNR {}", if *enabled { "on" } else { "off" }))
        }
        Setting::Antivirus { required } => {
            let mut running = false;
            for u in ["clamav-daemon", "clamd@scan", "clamav-clamonacc"] {
                running |= run("systemctl", &["is-active", u]).await?.stdout.trim() == "active";
            }
            if running {
                item(s, Compliant, "ClamAV is running")
            } else if *required {
                item(s, NonCompliant, "No running antivirus found (ClamAV)")
            } else {
                item(s, Compliant, "Not enforced")
            }
        }
        Setting::AppReputation { .. } => item(s, Unsupported, "Linux has no built-in app reputation service"),
        Setting::SecurityAuditing { enabled } => {
            if !*enabled {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            if !unit_exists("auditd.service").await {
                return Ok(item(s, NonCompliant, "auditd isn't installed (package auditd or audit)"));
            }
            let active = run("systemctl", &["is-active", "auditd"]).await?.stdout.trim() == "active";
            if active {
                item(s, Compliant, "auditd running")
            } else {
                ok_or_failed(s, set_service("auditd", true).await?, "auditd turned on", "Couldn't start auditd")
            }
        }
        Setting::UpdateDeferral { .. } => item(s, Unsupported, "Use a scheduled script from the Script Library to stage Linux updates"),
        _ => item(s, Unsupported, "Not supported by this agent version"),
    })
}

// ====================================================================== macOS
#[cfg(target_os = "macos")]
async fn pwpolicy(s: &Setting, kv: &str, good: String) -> anyhow::Result<ComplianceItem> {
    let o = run("pwpolicy", &["-setglobalpolicy", kv]).await?;
    Ok(ok_or_failed(s, o.ok(), good, o.message()))
}

#[cfg(target_os = "macos")]
pub async fn apply(s: &Setting) -> anyhow::Result<ComplianceItem> {
    use std::path::Path;
    use ComplianceState::*;
    const LW: &str = "/Library/Preferences/com.apple.loginwindow";
    Ok(match s {
        Setting::AccountLockout { threshold, duration_minutes } => {
            pwpolicy(
                s,
                &format!("maxFailedLoginAttempts={threshold} minutesUntilFailedLoginReset={duration_minutes}"),
                format!("Lock after {threshold} failures for {duration_minutes} min"),
            )
            .await?
        }
        Setting::PasswordMaxAge { days } => {
            pwpolicy(
                s,
                &format!("maxMinutesUntilChangePassword={}", days * 1440),
                if *days == 0 { "Passwords don't expire".into() } else { format!("Passwords expire after {days} days") },
            )
            .await?
        }
        Setting::PasswordHistory { count } => pwpolicy(s, &format!("usingHistory={count}"), format!("Remembers {count} passwords")).await?,
        Setting::PasswordComplexity { required } => {
            let v = if *required { 1 } else { 0 };
            pwpolicy(
                s,
                &format!("requiresAlpha={v} requiresNumeric={v}"),
                if *required { "Letters and numbers required".into() } else { "Not enforced".into() },
            )
            .await?
        }
        Setting::UsbStorage { mode } if mode == "allow" => item(s, Compliant, "USB storage allowed"),
        Setting::UsbStorage { .. } => item(s, Unsupported, "Restricting USB storage on macOS needs an MDM configuration profile"),
        Setting::GuestAccount { enabled } => {
            let o = run("defaults", &["write", LW, "GuestEnabled", "-bool", if *enabled { "true" } else { "false" }]).await?;
            ok_or_failed(s, o.ok(), if *enabled { "Guest user on" } else { "Guest user off" }, o.message())
        }
        Setting::RemoteDesktop { enabled } => {
            let svc = "system/com.apple.screensharing";
            let o = if *enabled { run("launchctl", &["enable", svc]).await? } else { run("launchctl", &["disable", svc]).await? };
            if !*enabled {
                let _ = run("launchctl", &["bootout", svc]).await;
            }
            ok_or_failed(s, o.ok(), if *enabled { "Screen Sharing on" } else { "Screen Sharing off" }, o.message())
        }
        Setting::SshServer { enabled } => {
            let o = run("systemsetup", &["-f", "-setremotelogin", if *enabled { "on" } else { "off" }]).await?;
            if o.stdout.contains("Full Disk Access") || o.stderr.contains("Full Disk Access") {
                item(s, Failed, "Give the Endpoint Agent Full Disk Access to change Remote Login")
            } else {
                ok_or_failed(s, o.ok(), if *enabled { "Remote Login on" } else { "Remote Login off" }, o.message())
            }
        }
        Setting::AutoLogin { allowed } => {
            if *allowed {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            let had = run("defaults", &["read", LW, "autoLoginUser"]).await?.ok() || Path::new("/etc/kcpassword").exists();
            let _ = run("defaults", &["delete", LW, "autoLoginUser"]).await;
            let _ = std::fs::remove_file("/etc/kcpassword");
            item(s, if had { Applied } else { Compliant }, "Automatic login off")
        }
        Setting::TimeServer { server } => {
            let a = run("systemsetup", &["-setnetworktimeserver", server]).await?;
            let _ = run("systemsetup", &["-setusingnetworktime", "on"]).await;
            ok_or_failed(s, a.ok(), format!("Network time from {server}"), a.message())
        }
        Setting::HideLastUsername { enabled } => {
            let o = run("defaults", &["write", LW, "SHOWFULLNAME", "-bool", if *enabled { "true" } else { "false" }]).await?;
            ok_or_failed(
                s,
                o.ok(),
                if *enabled { "Login window asks for name and password" } else { "Login window shows a user list" },
                o.message(),
            )
        }
        Setting::DisableAutorun { .. } => item(s, Compliant, "macOS never runs programs from inserted media"),
        Setting::Smb1 { .. } => item(s, Compliant, "macOS file sharing doesn't offer SMBv1"),
        Setting::Llmnr { .. } => item(s, Compliant, "macOS doesn't use LLMNR"),
        Setting::Antivirus { .. } => {
            let xp = Path::new("/Library/Apple/System/Library/CoreServices/XProtect.bundle").exists()
                || Path::new("/System/Library/CoreServices/XProtect.bundle").exists();
            if xp {
                item(s, Compliant, "XProtect is present")
            } else {
                item(s, NonCompliant, "XProtect not found")
            }
        }
        Setting::AppReputation { enabled } => {
            if !*enabled {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            let st = run("spctl", &["--status"]).await?;
            if st.stdout.contains("enabled") {
                return Ok(item(s, Compliant, "Gatekeeper on"));
            }
            let mut o = run("spctl", &["--global-enable"]).await?;
            if !o.ok() {
                o = run("spctl", &["--master-enable"]).await?;
            }
            if o.ok() {
                item(s, Applied, "Gatekeeper turned on")
            } else {
                item(s, Failed, o.message())
            }
        }
        Setting::SecurityAuditing { .. } => item(s, Compliant, "The unified log records sign-ins and account changes"),
        Setting::UpdateDeferral { .. } => item(s, Unsupported, "Deferring macOS updates needs an MDM profile"),
        _ => item(s, Unsupported, "Not supported by this agent version"),
    })
}

// ==================================================================== Windows
#[cfg(target_os = "windows")]
async fn reg_dword(key: &str, name: &str, value: u32) -> anyhow::Result<bool> {
    Ok(run("reg.exe", &["add", key, "/v", name, "/t", "REG_DWORD", "/d", &value.to_string(), "/f"]).await?.ok())
}

#[cfg(target_os = "windows")]
async fn ps_ok(script: &str) -> anyhow::Result<crate::util::Output> {
    crate::util::powershell(script, None).await
}

#[cfg(target_os = "windows")]
pub async fn apply(s: &Setting) -> anyhow::Result<ComplianceItem> {
    use ComplianceState::*;
    const SYSTEM: &str = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
    let net = |args: Vec<String>| async move {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        run("net.exe", &a).await
    };
    Ok(match s {
        Setting::AccountLockout { threshold, duration_minutes } => {
            let o = net(vec![
                "accounts".into(),
                format!("/lockoutthreshold:{threshold}"),
                format!("/lockoutwindow:{duration_minutes}"),
                format!("/lockoutduration:{duration_minutes}"),
            ])
            .await?;
            ok_or_failed(s, o.ok(), format!("Lock after {threshold} failures for {duration_minutes} min"), o.message())
        }
        Setting::PasswordMaxAge { days } => {
            let v = if *days == 0 { "UNLIMITED".to_string() } else { days.min(&999).to_string() };
            let o = net(vec!["accounts".into(), format!("/maxpwage:{v}")]).await?;
            ok_or_failed(
                s,
                o.ok(),
                if *days == 0 { "Passwords don't expire".into() } else { format!("Maximum password age {days} days") },
                o.message(),
            )
        }
        Setting::PasswordHistory { count } => {
            let n = (*count).min(24);
            let o = net(vec!["accounts".into(), format!("/uniquepw:{n}")]).await?;
            ok_or_failed(s, o.ok(), format!("Remembers {n} passwords"), o.message())
        }
        Setting::PasswordComplexity { required } => {
            let v = if *required { 1 } else { 0 };
            let script = format!(
                r#"$d = Join-Path $env:TEMP "od-secpol"; New-Item -ItemType Directory -Force $d | Out-Null
secedit /export /cfg "$d\cur.inf" /areas SECURITYPOLICY | Out-Null
$c = Get-Content "$d\cur.inf"
if ($c -match '^PasswordComplexity = {v}$') {{ 'same'; exit 0 }}
$c = $c -replace '^PasswordComplexity = \d', 'PasswordComplexity = {v}'
$c | Set-Content "$d\new.inf"
secedit /configure /db "$d\od.sdb" /cfg "$d\new.inf" /areas SECURITYPOLICY | Out-Null
Remove-Item -Recurse -Force $d
'changed'"#
            );
            let o = ps_ok(&script).await?;
            if !o.ok() {
                item(s, Failed, o.message())
            } else {
                item(
                    s,
                    if o.stdout.contains("changed") { Applied } else { Compliant },
                    if *required { "Complexity required" } else { "Complexity not required" },
                )
            }
        }
        Setting::UsbStorage { mode } => {
            let key = r"HKLM\SOFTWARE\Policies\Microsoft\Windows\RemovableStorageDevices";
            let disk = format!(r"{key}\{{53f5630d-b6bf-11d0-94f2-00a0c91efb8b}}");
            let ok = match mode.as_str() {
                "block" => reg_dword(key, "Deny_All", 1).await?,
                "read_only" => {
                    let _ = run("reg.exe", &["delete", key, "/v", "Deny_All", "/f"]).await;
                    reg_dword(&disk, "Deny_Write", 1).await?
                }
                _ => {
                    let _ = run("reg.exe", &["delete", key, "/v", "Deny_All", "/f"]).await;
                    let _ = run("reg.exe", &["delete", &disk, "/v", "Deny_Write", "/f"]).await;
                    true
                }
            };
            let _ = run("gpupdate.exe", &["/target:computer", "/force"]).await;
            ok_or_failed(
                s,
                ok,
                match mode.as_str() {
                    "block" => "Removable storage blocked",
                    "read_only" => "Removable disks read-only",
                    _ => "USB storage allowed",
                },
                "Couldn't write the removable storage policy",
            )
        }
        Setting::GuestAccount { enabled } => {
            let verb = if *enabled { "Enable-LocalUser" } else { "Disable-LocalUser" };
            let o = ps_ok(&format!("Get-LocalUser | Where-Object {{ $_.SID -like '*-501' }} | {verb}")).await?;
            ok_or_failed(s, o.ok(), if *enabled { "Guest account on" } else { "Guest account off" }, o.message())
        }
        Setting::RemoteDesktop { enabled } => {
            let ok =
                reg_dword(r"HKLM\SYSTEM\CurrentControlSet\Control\Terminal Server", "fDenyTSConnections", if *enabled { 0 } else { 1 })
                    .await?;
            let verb = if *enabled { "Enable-NetFirewallRule" } else { "Disable-NetFirewallRule" };
            let _ = ps_ok(&format!("{verb} -Group '@FirewallAPI.dll,-28752' -ErrorAction SilentlyContinue")).await;
            ok_or_failed(s, ok, if *enabled { "Remote Desktop on" } else { "Remote Desktop off" }, "Couldn't write fDenyTSConnections")
        }
        Setting::SshServer { enabled } => {
            let script = if *enabled {
                "if (-not (Get-Service sshd -ErrorAction SilentlyContinue)) { Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0 | Out-Null }\nSet-Service sshd -StartupType Automatic; Start-Service sshd; 'ok'"
            } else {
                "if (Get-Service sshd -ErrorAction SilentlyContinue) { Stop-Service sshd -Force; Set-Service sshd -StartupType Disabled }; 'ok'"
            };
            let o = crate::util::run_with(
                "powershell.exe",
                &["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script],
                None,
                std::time::Duration::from_secs(900),
            )
            .await?;
            ok_or_failed(s, o.ok(), if *enabled { "OpenSSH Server running" } else { "OpenSSH Server off" }, o.message())
        }
        Setting::AutoLogin { allowed } => {
            if *allowed {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            let key = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon";
            let o = run("reg.exe", &["add", key, "/v", "AutoAdminLogon", "/t", "REG_SZ", "/d", "0", "/f"]).await?;
            let _ = run("reg.exe", &["delete", key, "/v", "DefaultPassword", "/f"]).await;
            ok_or_failed(s, o.ok(), "AutoAdminLogon off", o.message())
        }
        Setting::TimeServer { server } => {
            let _ = run("sc.exe", &["config", "w32time", "start=", "auto"]).await;
            let _ = run("net.exe", &["start", "w32time"]).await;
            let o = run("w32tm.exe", &["/config", &format!("/manualpeerlist:{server},0x8"), "/syncfromflags:manual", "/update"]).await?;
            let _ = run("w32tm.exe", &["/resync", "/nowait"]).await;
            ok_or_failed(s, o.ok(), format!("Windows Time uses {server}"), o.message())
        }
        Setting::HideLastUsername { enabled } => {
            let ok = reg_dword(SYSTEM, "DontDisplayLastUserName", if *enabled { 1 } else { 0 }).await?;
            ok_or_failed(
                s,
                ok,
                if *enabled { "Last user name hidden" } else { "Last user name shown" },
                "Couldn't write DontDisplayLastUserName",
            )
        }
        Setting::DisableAutorun { enabled } => {
            if !*enabled {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            let key = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Explorer";
            let ok = reg_dword(key, "NoDriveTypeAutoRun", 255).await? && reg_dword(key, "NoAutorun", 1).await?;
            ok_or_failed(s, ok, "AutoRun off for all drives", "Couldn't write the AutoRun policy")
        }
        Setting::Smb1 { enabled } => {
            let v = if *enabled { "$true" } else { "$false" };
            let o = ps_ok(&format!(
                "$c = (Get-SmbServerConfiguration).EnableSMB1Protocol; if ($c -eq {v}) {{ 'same' }} else {{ Set-SmbServerConfiguration -EnableSMB1Protocol {v} -Force -Confirm:$false; 'changed' }}"
            ))
            .await?;
            if !o.ok() {
                item(s, Failed, o.message())
            } else {
                item(
                    s,
                    if o.stdout.contains("changed") { Applied } else { Compliant },
                    if *enabled { "SMBv1 server on" } else { "SMBv1 server off" },
                )
            }
        }
        Setting::Llmnr { enabled } => {
            let ok = reg_dword(r"HKLM\SOFTWARE\Policies\Microsoft\Windows NT\DNSClient", "EnableMulticast", if *enabled { 1 } else { 0 })
                .await?;
            ok_or_failed(s, ok, if *enabled { "LLMNR on" } else { "LLMNR off" }, "Couldn't write EnableMulticast")
        }
        Setting::Antivirus { required } => {
            let o = ps_ok("try { (Get-MpComputerStatus -ErrorAction Stop).RealTimeProtectionEnabled } catch { 'Unavailable' }").await?;
            match o.stdout.trim() {
                "True" => item(s, Compliant, "Microsoft Defender real-time protection on"),
                "Unavailable" => item(
                    s,
                    if *required { NonCompliant } else { Compliant },
                    "Microsoft Defender isn't available (another antivirus may be in charge)",
                ),
                _ if !*required => item(s, Compliant, "Not enforced"),
                _ => {
                    let r = ps_ok("Set-MpPreference -DisableRealtimeMonitoring $false").await?;
                    if r.ok() {
                        item(s, Applied, "Real-time protection turned on")
                    } else {
                        item(s, Failed, format!("Couldn't turn on real-time protection (Tamper Protection may block it): {}", r.message()))
                    }
                }
            }
        }
        Setting::AppReputation { enabled } => {
            let key = r"HKLM\SOFTWARE\Policies\Microsoft\Windows\System";
            let ok = reg_dword(key, "EnableSmartScreen", if *enabled { 1 } else { 0 }).await?;
            if *enabled {
                let _ = run("reg.exe", &["add", key, "/v", "ShellSmartScreenLevel", "/t", "REG_SZ", "/d", "Warn", "/f"]).await;
            }
            ok_or_failed(s, ok, if *enabled { "SmartScreen on" } else { "SmartScreen not enforced" }, "Couldn't write EnableSmartScreen")
        }
        Setting::SecurityAuditing { enabled } => {
            if !*enabled {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            // Subcategory GUIDs avoid localized names: Logon, Logoff, Account Lockout, User Account Management, Security Group Management.
            let mut ok = true;
            for g in [
                "{0CCE9215-69AE-11D9-BED3-505054503030}",
                "{0CCE9216-69AE-11D9-BED3-505054503030}",
                "{0CCE9217-69AE-11D9-BED3-505054503030}",
                "{0CCE9235-69AE-11D9-BED3-505054503030}",
                "{0CCE9237-69AE-11D9-BED3-505054503030}",
            ] {
                ok &= run("auditpol.exe", &["/set", &format!("/subcategory:{g}"), "/success:enable", "/failure:enable"]).await?.ok();
            }
            ok_or_failed(s, ok, "Logon and account management auditing on", "auditpol failed")
        }
        Setting::UpdateDeferral { quality_days, feature_days } => {
            let key = r"HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate";
            let q = (*quality_days).min(30);
            let f = (*feature_days).min(365);
            let ok = reg_dword(key, "DeferQualityUpdates", u32::from(q > 0)).await?
                && reg_dword(key, "DeferQualityUpdatesPeriodInDays", q).await?
                && reg_dword(key, "DeferFeatureUpdates", u32::from(f > 0)).await?
                && reg_dword(key, "DeferFeatureUpdatesPeriodInDays", f).await?;
            ok_or_failed(
                s,
                ok,
                format!("Quality updates deferred {q} d, feature updates {f} d"),
                "Couldn't write Windows Update for Business policy",
            )
        }
        _ => item(s, Unsupported, "Not supported by this agent version"),
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub async fn apply(s: &Setting) -> anyhow::Result<ComplianceItem> {
    Ok(item(s, ComplianceState::Unsupported, "Unsupported OS"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conf_and_ini_keys() {
        let dir = std::env::temp_dir().join(format!("od-ext-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("faillock.conf");
        std::fs::write(&p, "# deny = 3\nfoo = 1\n").unwrap();
        assert!(set_conf_key(&p, "deny", " = ", "5").unwrap());
        assert!(!set_conf_key(&p, "deny", " = ", "5").unwrap());
        assert!(set_conf_key(&p, "unlock_time", " = ", "900").unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "deny = 5\nfoo = 1\nunlock_time = 900\n");

        let ini = dir.join("custom.conf");
        std::fs::write(&ini, "[daemon]\n#AutomaticLoginEnable=true\n[security]\n").unwrap();
        assert!(set_ini_key(&ini, "daemon", "AutomaticLoginEnable", "false").unwrap());
        assert_eq!(std::fs::read_to_string(&ini).unwrap(), "[daemon]\nAutomaticLoginEnable=false\n[security]\n");
        assert!(set_ini_key(&ini, "global", "server min protocol", "SMB2").unwrap());
        assert!(std::fs::read_to_string(&ini).unwrap().ends_with("[global]\nserver min protocol=SMB2\n"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
