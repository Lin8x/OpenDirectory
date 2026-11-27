//! Applies cross-platform policy settings with each OS's native mechanism and
//! reports compliance. Settings removed from policy are left as they are
//! (like GPO "tattooing"); a later release will add revert support.

use od_core::policy::Setting;
use od_core::{ComplianceItem, ComplianceState};

#[allow(unused_imports)]
use crate::util::{ensure_file, run, which};

fn item(s: &Setting, state: ComplianceState, message: impl Into<String>) -> ComplianceItem {
    ComplianceItem { setting: s.kind().to_string(), state, message: message.into() }
}

pub async fn apply_all(settings: &[Setting]) -> Vec<ComplianceItem> {
    let mut out = Vec::new();
    let mut packages: Vec<String> = Vec::new();
    let mut deploy: Vec<od_core::manage::PackageRule> = Vec::new();
    for s in settings {
        match s {
            Setting::InstallPackages { packages: p } => packages.extend(p.iter().cloned()),
            Setting::LocalAccounts { .. }
            | Setting::RunScript { .. }
            | Setting::LocalAdminPassword { .. }
            | Setting::CertificateAutoEnrollment { .. }
            | Setting::ConsentNotice { .. }
            | Setting::HelpDesk { .. } => {}
            // Enforced by the agent itself when a session is requested.
            Setting::SoftwareDeployment { packages: p } => deploy.extend(p.iter().cloned()),
            Setting::RemoteSessionConsent { .. } | Setting::RemoteTools { .. } | Setting::ManagementTools { .. } => {
                out.push(item(s, ComplianceState::Compliant, "Enforced by the Endpoint Agent"))
            }
            other => {
                let r = apply(other).await;
                out.push(match r {
                    Ok(i) => i,
                    Err(e) => item(other, ComplianceState::Failed, format!("{e:#}")),
                });
            }
        }
    }
    if !deploy.is_empty() {
        out.push(crate::manage::software::deploy(&deploy).await);
    }
    if !packages.is_empty() {
        packages.sort();
        packages.dedup();
        let s = Setting::InstallPackages { packages: packages.clone() };
        out.push(match install_packages(&packages).await {
            Ok(i) => i,
            Err(e) => item(&s, ComplianceState::Failed, format!("{e:#}")),
        });
    }
    out
}

// ====================================================================== Linux
#[cfg(target_os = "linux")]
pub async fn linux_root_encrypted() -> Option<bool> {
    let src = run("findmnt", &["-n", "-o", "SOURCE", "/"]).await.ok()?;
    let dev = src.stdout.trim().split('[').next()?.to_string();
    if dev.is_empty() {
        return None;
    }
    let o = run("lsblk", &["-s", "-n", "-r", "-o", "TYPE", &dev]).await.ok()?;
    if !o.ok() {
        return None;
    }
    Some(o.stdout.lines().any(|l| l.trim() == "crypt"))
}

#[cfg(target_os = "linux")]
pub async fn linux_firewall_active() -> Option<bool> {
    if which("ufw") {
        return run("ufw", &["status"]).await.ok().map(|o| o.stdout.contains("Status: active"));
    }
    if which("firewall-cmd") {
        return run("firewall-cmd", &["--state"]).await.ok().map(|o| o.stdout.trim() == "running");
    }
    None
}

#[cfg(target_os = "linux")]
async fn apply(s: &Setting) -> anyhow::Result<ComplianceItem> {
    use std::path::Path;
    use ComplianceState::*;
    Ok(match s {
        Setting::PasswordMinLength { length } => {
            let changed = ensure_file(
                Path::new("/etc/security/pwquality.conf.d/50-opendirectory.conf"),
                &format!("# Managed by OpenDirectory\nminlen = {length}\n"),
            )?;
            item(s, if changed { Applied } else { Compliant }, format!("pam_pwquality minlen = {length}"))
        }
        Setting::ScreenLock { minutes } => {
            if !Path::new("/etc/dconf").exists() || !which("dconf") {
                return Ok(item(s, Unsupported, "No GNOME/dconf desktop found"));
            }
            let profile = Path::new("/etc/dconf/profile/user");
            if !profile.exists() {
                ensure_file(profile, "user-db:user\nsystem-db:local\n")?;
            }
            let secs = minutes * 60;
            let a = ensure_file(
                Path::new("/etc/dconf/db/local.d/50-opendirectory-screenlock"),
                &format!("# Managed by OpenDirectory\n[org/gnome/desktop/session]\nidle-delay=uint32 {secs}\n\n[org/gnome/desktop/screensaver]\nlock-enabled=true\nlock-delay=uint32 0\n"),
            )?;
            let b = ensure_file(
                Path::new("/etc/dconf/db/local.d/locks/50-opendirectory-screenlock"),
                "/org/gnome/desktop/session/idle-delay\n/org/gnome/desktop/screensaver/lock-enabled\n/org/gnome/desktop/screensaver/lock-delay\n",
            )?;
            if a || b {
                run("dconf", &["update"]).await?;
            }
            item(s, if a || b { Applied } else { Compliant }, format!("GNOME locks after {minutes} min"))
        }
        Setting::Firewall { enabled } => {
            if !enabled {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            if linux_firewall_active().await == Some(true) {
                return Ok(item(s, Compliant, "Firewall active"));
            }
            if which("ufw") {
                let o = run("ufw", &["--force", "enable"]).await?;
                if !o.ok() {
                    return Ok(item(s, Failed, o.message()));
                }
            } else if which("firewall-cmd") || Path::new("/usr/lib/systemd/system/firewalld.service").exists() {
                let o = run("systemctl", &["enable", "--now", "firewalld"]).await?;
                if !o.ok() {
                    return Ok(item(s, Failed, o.message()));
                }
            } else {
                return Ok(item(s, NonCompliant, "Neither ufw nor firewalld is installed"));
            }
            if linux_firewall_active().await == Some(true) {
                item(s, Applied, "Firewall turned on")
            } else {
                item(s, Failed, "Firewall did not start")
            }
        }
        Setting::DiskEncryption { required } => match linux_root_encrypted().await {
            Some(true) => item(s, Compliant, "Root filesystem is on LUKS"),
            Some(false) if *required => item(s, NonCompliant, "Root filesystem is not encrypted"),
            Some(false) => item(s, Compliant, "Not encrypted (optional)"),
            None => item(s, NonCompliant, "Couldn't determine encryption state"),
        },
        Setting::AutomaticUpdates { enabled } => {
            if !enabled {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            if which("apt-get") {
                if !which("unattended-upgrade") {
                    let o = crate::util::run_with(
                        "apt-get",
                        &["install", "-y", "unattended-upgrades"],
                        None,
                        std::time::Duration::from_secs(900),
                    )
                    .await?;
                    if !o.ok() {
                        return Ok(item(s, Failed, format!("Installing unattended-upgrades: {}", o.message())));
                    }
                }
                let changed = ensure_file(
                    Path::new("/etc/apt/apt.conf.d/20auto-upgrades"),
                    "// Managed by OpenDirectory\nAPT::Periodic::Update-Package-Lists \"1\";\nAPT::Periodic::Unattended-Upgrade \"1\";\n",
                )?;
                item(s, if changed { Applied } else { Compliant }, "unattended-upgrades enabled")
            } else if which("dnf") {
                let timer = run("systemctl", &["is-enabled", "dnf-automatic-install.timer"]).await?;
                if timer.ok() {
                    return Ok(item(s, Compliant, "dnf-automatic installing updates"));
                }
                let o =
                    crate::util::run_with("dnf", &["install", "-y", "dnf-automatic"], None, std::time::Duration::from_secs(900)).await?;
                if !o.ok() {
                    return Ok(item(s, Failed, format!("Installing dnf-automatic: {}", o.message())));
                }
                let o = run("systemctl", &["enable", "--now", "dnf-automatic-install.timer"]).await?;
                if o.ok() {
                    item(s, Applied, "dnf-automatic enabled")
                } else {
                    item(s, Failed, o.message())
                }
            } else {
                item(s, Unsupported, "Only apt and dnf based distributions are supported")
            }
        }
        Setting::LoginBanner { title, text } => {
            let body = format!("{title}\n\n{text}\n\n");
            let a = ensure_file(Path::new("/etc/issue"), &body)?;
            let b = ensure_file(Path::new("/etc/issue.net"), &body)?;
            item(s, if a || b { Applied } else { Compliant }, "Shown on console and SSH login")
        }
        Setting::InstallPackages { .. } | Setting::LocalAccounts { .. } | Setting::RunScript { .. } => unreachable!("handled elsewhere"),
        _ => return crate::policy_ext::apply(s).await,
    })
}

#[cfg(target_os = "linux")]
async fn install_packages(pkgs: &[String]) -> anyhow::Result<ComplianceItem> {
    use std::time::Duration;
    let s = Setting::InstallPackages { packages: pkgs.to_vec() };
    #[allow(clippy::type_complexity)]
    let (check, install): (fn(&str) -> (String, Vec<String>), (&str, Vec<&str>)) = if which("apt-get") {
        (|p| ("dpkg-query".into(), vec!["-W".into(), "-f=${Status}".into(), p.into()]), ("apt-get", vec!["install", "-y"]))
    } else if which("dnf") {
        (|p| ("rpm".into(), vec!["-q".into(), p.into()]), ("dnf", vec!["install", "-y"]))
    } else if which("zypper") {
        (|p| ("rpm".into(), vec!["-q".into(), p.into()]), ("zypper", vec!["--non-interactive", "install"]))
    } else if which("pacman") {
        (|p| ("pacman".into(), vec!["-Q".into(), p.into()]), ("pacman", vec!["-S", "--noconfirm"]))
    } else {
        return Ok(item(&s, ComplianceState::Unsupported, "No supported package manager found"));
    };
    let mut missing = Vec::new();
    for p in pkgs {
        let (prog, args) = check(p);
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let o = run(&prog, &a).await?;
        let installed = o.ok() && (prog != "dpkg-query" || o.stdout.contains("install ok installed"));
        if !installed {
            missing.push(p.as_str());
        }
    }
    if missing.is_empty() {
        return Ok(item(&s, ComplianceState::Compliant, format!("{} package(s) present", pkgs.len())));
    }
    let mut args = install.1.clone();
    args.extend(missing.iter().copied());
    let o = crate::util::run_with(install.0, &args, None, Duration::from_secs(1800)).await?;
    Ok(if o.ok() {
        item(&s, ComplianceState::Applied, format!("Installed {}", missing.join(", ")))
    } else {
        item(&s, ComplianceState::Failed, o.message())
    })
}

// ====================================================================== macOS
#[cfg(target_os = "macos")]
async fn apply(s: &Setting) -> anyhow::Result<ComplianceItem> {
    use std::path::Path;
    use ComplianceState::*;
    const FW: &str = "/usr/libexec/ApplicationFirewall/socketfilterfw";
    Ok(match s {
        Setting::PasswordMinLength { length } => {
            let o = run("pwpolicy", &["-setglobalpolicy", &format!("minChars={length}")]).await?;
            if o.ok() {
                item(s, Compliant, format!("pwpolicy minChars={length}"))
            } else {
                item(s, Failed, o.message())
            }
        }
        Setting::ScreenLock { minutes } => {
            let secs = (minutes * 60).to_string();
            let a = run("defaults", &["write", "/Library/Preferences/com.apple.screensaver", "idleTime", "-int", &secs]).await?;
            let b = run("defaults", &["write", "/Library/Preferences/com.apple.screensaver", "askForPassword", "-int", "1"]).await?;
            let c = run("defaults", &["write", "/Library/Preferences/com.apple.screensaver", "askForPasswordDelay", "-int", "0"]).await?;
            if a.ok() && b.ok() && c.ok() {
                item(s, Compliant, format!("Screen saver after {minutes} min, password required"))
            } else {
                item(s, Failed, a.message())
            }
        }
        Setting::Firewall { enabled } => {
            if !enabled {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            let st = run(FW, &["--getglobalstate"]).await?;
            if st.stdout.contains("enabled") {
                return Ok(item(s, Compliant, "Application Firewall on"));
            }
            let o = run(FW, &["--setglobalstate", "on"]).await?;
            if o.ok() {
                item(s, Applied, "Application Firewall turned on")
            } else {
                item(s, Failed, o.message())
            }
        }
        Setting::DiskEncryption { required } => {
            let o = run("fdesetup", &["status"]).await?;
            if o.stdout.contains("FileVault is On") {
                item(s, Compliant, "FileVault is on")
            } else if *required {
                item(s, NonCompliant, "FileVault is off")
            } else {
                item(s, Compliant, "FileVault off (optional)")
            }
        }
        Setting::AutomaticUpdates { enabled } => {
            if !enabled {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            let p = "/Library/Preferences/com.apple.SoftwareUpdate";
            let mut ok = true;
            for key in [
                "AutomaticCheckEnabled",
                "AutomaticDownload",
                "CriticalUpdateInstall",
                "ConfigDataInstall",
                "AutomaticallyInstallMacOSUpdates",
            ] {
                ok &= run("defaults", &["write", p, key, "-bool", "true"]).await?.ok();
            }
            ok &= run("defaults", &["write", "/Library/Preferences/com.apple.commerce", "AutoUpdate", "-bool", "true"]).await?.ok();
            if ok {
                item(s, Compliant, "Automatic macOS and security updates on")
            } else {
                item(s, Failed, "Couldn't write Software Update preferences")
            }
        }
        Setting::LoginBanner { title, text } => {
            let changed = ensure_file(Path::new("/Library/Security/PolicyBanner.txt"), &format!("{title}\n\n{text}\n"))?;
            item(s, if changed { Applied } else { Compliant }, "Shown at the login window")
        }
        Setting::InstallPackages { .. } | Setting::LocalAccounts { .. } | Setting::RunScript { .. } => unreachable!("handled elsewhere"),
        _ => return crate::policy_ext::apply(s).await,
    })
}

#[cfg(target_os = "macos")]
async fn install_packages(pkgs: &[String]) -> anyhow::Result<ComplianceItem> {
    use std::os::unix::fs::MetadataExt;
    let s = Setting::InstallPackages { packages: pkgs.to_vec() };
    let brew = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].into_iter().find(|p| std::path::Path::new(p).exists());
    let Some(brew) = brew else {
        return Ok(item(&s, ComplianceState::Unsupported, "Homebrew is not installed"));
    };
    // Homebrew refuses to run as root; run it as the user who owns it.
    let uid = std::fs::metadata(brew)?.uid();
    let owner = run("id", &["-nu", &uid.to_string()]).await?.stdout.trim().to_string();
    let mut missing = Vec::new();
    for p in pkgs {
        if !run("sudo", &["-u", &owner, brew, "list", p]).await?.ok() {
            missing.push(p.as_str());
        }
    }
    if missing.is_empty() {
        return Ok(item(&s, ComplianceState::Compliant, format!("{} package(s) present", pkgs.len())));
    }
    let mut args = vec!["-u", owner.as_str(), brew, "install"];
    args.extend(missing.iter().copied());
    let o = crate::util::run_with("sudo", &args, None, std::time::Duration::from_secs(1800)).await?;
    Ok(if o.ok() {
        item(&s, ComplianceState::Applied, format!("Installed {}", missing.join(", ")))
    } else {
        item(&s, ComplianceState::Failed, o.message())
    })
}

// ==================================================================== Windows
#[cfg(target_os = "windows")]
async fn reg_dword(key: &str, name: &str, value: u32) -> anyhow::Result<bool> {
    Ok(run("reg.exe", &["add", key, "/v", name, "/t", "REG_DWORD", "/d", &value.to_string(), "/f"]).await?.ok())
}

#[cfg(target_os = "windows")]
async fn apply(s: &Setting) -> anyhow::Result<ComplianceItem> {
    use crate::util::powershell;
    use ComplianceState::*;
    const SYSTEM: &str = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
    Ok(match s {
        Setting::PasswordMinLength { length } => {
            let o = run("net.exe", &["accounts", &format!("/minpwlen:{}", (*length).min(14))]).await?;
            let note = if *length > 14 { " (Windows local policy caps at 14)" } else { "" };
            if o.ok() {
                item(s, Compliant, format!("Minimum password length {}{note}", (*length).min(14)))
            } else {
                item(s, Failed, o.message())
            }
        }
        Setting::ScreenLock { minutes } => {
            if reg_dword(SYSTEM, "InactivityTimeoutSecs", minutes * 60).await? {
                item(s, Compliant, format!("Machine inactivity limit {minutes} min"))
            } else {
                item(s, Failed, "Couldn't write InactivityTimeoutSecs")
            }
        }
        Setting::Firewall { enabled } => {
            if !enabled {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            let st = run("netsh.exe", &["advfirewall", "show", "allprofiles", "state"]).await?;
            if st.stdout.matches("OFF").count() == 0 && st.stdout.contains("ON") {
                return Ok(item(s, Compliant, "All firewall profiles on"));
            }
            let o = run("netsh.exe", &["advfirewall", "set", "allprofiles", "state", "on"]).await?;
            if o.ok() {
                item(s, Applied, "Firewall turned on for all profiles")
            } else {
                item(s, Failed, o.message())
            }
        }
        Setting::DiskEncryption { required } => {
            let o = powershell(
                "try { (Get-BitLockerVolume -MountPoint $env:SystemDrive -ErrorAction Stop).ProtectionStatus } catch { 'Unavailable' }",
                None,
            )
            .await?;
            match o.stdout.trim() {
                "On" => item(s, Compliant, "BitLocker protection on"),
                "Unavailable" => item(
                    s,
                    if *required { NonCompliant } else { Compliant },
                    "BitLocker isn't available on this edition; check Device Encryption in Settings",
                ),
                _ if *required => item(s, NonCompliant, "BitLocker is off"),
                _ => item(s, Compliant, "BitLocker off (optional)"),
            }
        }
        Setting::AutomaticUpdates { enabled } => {
            if !enabled {
                return Ok(item(s, Compliant, "Not enforced"));
            }
            let key = r"HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU";
            let ok = reg_dword(key, "NoAutoUpdate", 0).await? && reg_dword(key, "AUOptions", 4).await?;
            if ok {
                item(s, Compliant, "Automatic download and install")
            } else {
                item(s, Failed, "Couldn't write Windows Update policy")
            }
        }
        Setting::LoginBanner { title, text } => {
            let a = run("reg.exe", &["add", SYSTEM, "/v", "legalnoticecaption", "/t", "REG_SZ", "/d", title, "/f"]).await?;
            let b = run("reg.exe", &["add", SYSTEM, "/v", "legalnoticetext", "/t", "REG_SZ", "/d", text, "/f"]).await?;
            if a.ok() && b.ok() {
                item(s, Compliant, "Shown before sign-in")
            } else {
                item(s, Failed, a.message())
            }
        }
        Setting::InstallPackages { .. } | Setting::LocalAccounts { .. } | Setting::RunScript { .. } => unreachable!("handled elsewhere"),
        _ => return crate::policy_ext::apply(s).await,
    })
}

#[cfg(target_os = "windows")]
async fn install_packages(pkgs: &[String]) -> anyhow::Result<ComplianceItem> {
    let s = Setting::InstallPackages { packages: pkgs.to_vec() };
    // The SYSTEM account has no winget alias; find the App Installer binary directly.
    let find = r#"$w = Get-ChildItem "$env:ProgramFiles\WindowsApps\Microsoft.DesktopAppInstaller_*_x64__8wekyb3d8bbwe\winget.exe" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1; if ($w) { $w.FullName } elseif (Get-Command winget -ErrorAction SilentlyContinue) { 'winget' }"#;
    let w = crate::util::powershell(find, None).await?.stdout.trim().to_string();
    if w.is_empty() {
        return Ok(item(&s, ComplianceState::Unsupported, "winget (App Installer) is not installed"));
    }
    let mut missing = Vec::new();
    for p in pkgs {
        let o = run(&w, &["list", "--id", p, "--exact", "--accept-source-agreements"]).await?;
        if !o.ok() {
            missing.push(p.as_str());
        }
    }
    if missing.is_empty() {
        return Ok(item(&s, ComplianceState::Compliant, format!("{} package(s) present", pkgs.len())));
    }
    let mut failed = Vec::new();
    for p in &missing {
        let o = crate::util::run_with(
            &w,
            &[
                "install",
                "--id",
                p,
                "--exact",
                "--silent",
                "--scope",
                "machine",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ],
            None,
            std::time::Duration::from_secs(1800),
        )
        .await?;
        if !o.ok() {
            failed.push(format!("{p}: {}", o.message()));
        }
    }
    Ok(if failed.is_empty() {
        item(&s, ComplianceState::Applied, format!("Installed {}", missing.join(", ")))
    } else {
        item(&s, ComplianceState::Failed, failed.join("; "))
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
async fn apply(s: &Setting) -> anyhow::Result<ComplianceItem> {
    crate::policy_ext::apply(s).await
}
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
async fn install_packages(pkgs: &[String]) -> anyhow::Result<ComplianceItem> {
    Ok(item(&Setting::InstallPackages { packages: pkgs.to_vec() }, ComplianceState::Unsupported, "Unsupported OS"))
}
