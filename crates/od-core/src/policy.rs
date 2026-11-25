//! Cross-platform policy engine.
//!
//! A policy is a list of *intents* ("lock the screen after 10 minutes"), not
//! Windows registry keys. The agent on each OS turns each intent into that
//! OS's native mechanism. This is how OpenDirectory replaces Group Policy on
//! machines that can't use GPOs (macOS, Linux, Windows Home).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::OsFamily;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Setting {
    /// Minimum length for local account passwords.
    PasswordMinLength { length: u32 },
    /// Lock the screen after this many idle minutes.
    ScreenLock { minutes: u32 },
    /// Keep the host firewall turned on.
    Firewall { enabled: bool },
    /// Report whether the system disk is encrypted.
    DiskEncryption { required: bool },
    /// Keep automatic OS updates on.
    AutomaticUpdates { enabled: bool },
    /// Create local sign-in accounts for every member of a directory group.
    LocalAccounts { group_id: Uuid, admin: bool },
    /// Show a notice before sign-in.
    LoginBanner { title: String, text: String },
    /// Make sure these packages are installed (winget / Homebrew / apt / dnf).
    InstallPackages { packages: Vec<String> },
}

impl Setting {
    pub fn kind(&self) -> &'static str {
        match self {
            Setting::PasswordMinLength { .. } => "password_min_length",
            Setting::ScreenLock { .. } => "screen_lock",
            Setting::Firewall { .. } => "firewall",
            Setting::DiskEncryption { .. } => "disk_encryption",
            Setting::AutomaticUpdates { .. } => "automatic_updates",
            Setting::LocalAccounts { .. } => "local_accounts",
            Setting::LoginBanner { .. } => "login_banner",
            Setting::InstallPackages { .. } => "install_packages",
        }
    }

    /// Settings of these kinds add up across policies instead of the
    /// highest-priority one winning.
    pub fn accumulates(&self) -> bool {
        matches!(self, Setting::LocalAccounts { .. } | Setting::InstallPackages { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Setting::PasswordMinLength { length } => format!("Passwords at least {length} characters"),
            Setting::ScreenLock { minutes } => format!("Lock screen after {minutes} min idle"),
            Setting::Firewall { enabled } => {
                if *enabled { "Firewall on".into() } else { "Firewall not enforced".into() }
            }
            Setting::DiskEncryption { required } => {
                if *required { "Disk encryption required".into() } else { "Disk encryption optional".into() }
            }
            Setting::AutomaticUpdates { enabled } => {
                if *enabled { "Automatic updates on".into() } else { "Automatic updates not enforced".into() }
            }
            Setting::LocalAccounts { admin, .. } => {
                if *admin { "Sign-in accounts for a group (as admins)".into() } else { "Sign-in accounts for a group".into() }
            }
            Setting::LoginBanner { title, .. } => format!("Login notice: {title}"),
            Setting::InstallPackages { packages } => format!("Install {}", packages.join(", ")),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyTargets {
    /// Apply to every enrolled endpoint.
    #[serde(default)]
    pub all_endpoints: bool,
    /// Apply to endpoints in these OUs (and their child OUs).
    #[serde(default)]
    pub ou_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// Lower number wins when two policies set the same thing (like GPO link order).
    pub priority: i32,
    pub targets: PolicyTargets,
    pub settings: Vec<Setting>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// How a setting is enforced on an OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    /// The agent enforces it.
    Enforced,
    /// The agent checks and reports it but doesn't change it.
    ReportOnly,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformSupport {
    pub os: OsFamily,
    pub level: SupportLevel,
    /// The native mechanism used, in plain words.
    pub mechanism: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub kind: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    /// One plain-language sentence for newcomers.
    pub summary: &'static str,
    /// What a Windows admin would call it.
    pub ad_equivalent: &'static str,
    pub support: Vec<PlatformSupport>,
    /// A sensible default value, used by the editor.
    pub default: Setting,
}

fn ps(os: OsFamily, level: SupportLevel, mechanism: &'static str) -> PlatformSupport {
    PlatformSupport { os, level, mechanism }
}

/// Every setting the policy editor offers, with plain-language help and the
/// native mechanism used on each OS.
pub fn catalog() -> Vec<CatalogEntry> {
    use OsFamily::*;
    use SupportLevel::*;
    vec![
        CatalogEntry {
            kind: "local_accounts",
            title: "Sign-in accounts",
            category: "Accounts",
            summary: "Lets members of a directory group sign in to these computers with their OpenDirectory username and password. Works on Windows Home too.",
            ad_equivalent: "Domain logon / Restricted Groups",
            support: vec![
                ps(Windows, Enforced, "Local accounts via the LocalAccounts PowerShell module"),
                ps(Macos, Enforced, "Local accounts via sysadminctl and dseditgroup"),
                ps(Linux, Enforced, "Local accounts via useradd, chpasswd and the sudo/wheel group"),
            ],
            default: Setting::LocalAccounts { group_id: Uuid::nil(), admin: false },
        },
        CatalogEntry {
            kind: "password_min_length",
            title: "Minimum password length",
            category: "Accounts",
            summary: "Rejects new local passwords shorter than this. Longer passwords are the single best defense against guessing.",
            ad_equivalent: "Password Policy: Minimum password length",
            support: vec![
                ps(Windows, Enforced, "net accounts /minpwlen"),
                ps(Macos, Enforced, "pwpolicy global account policy"),
                ps(Linux, Enforced, "pam_pwquality minlen in /etc/security/pwquality.conf"),
            ],
            default: Setting::PasswordMinLength { length: 12 },
        },
        CatalogEntry {
            kind: "screen_lock",
            title: "Automatic screen lock",
            category: "Security",
            summary: "Locks the computer after it has been idle, so an unattended desk can't be used by someone else.",
            ad_equivalent: "Interactive logon: Machine inactivity limit",
            support: vec![
                ps(Windows, Enforced, "InactivityTimeoutSecs machine policy"),
                ps(Macos, Enforced, "sysadminctl -screenLock and the screensaver idle time"),
                ps(Linux, Enforced, "GNOME dconf system defaults (idle-delay, lock-enabled)"),
            ],
            default: Setting::ScreenLock { minutes: 10 },
        },
        CatalogEntry {
            kind: "firewall",
            title: "Firewall",
            category: "Security",
            summary: "Keeps the built-in firewall switched on so the computer doesn't accept unexpected network connections.",
            ad_equivalent: "Windows Defender Firewall: Protect all network connections",
            support: vec![
                ps(Windows, Enforced, "netsh advfirewall, all profiles"),
                ps(Macos, Enforced, "Application Firewall (socketfilterfw)"),
                ps(Linux, Enforced, "ufw or firewalld, whichever is installed"),
            ],
            default: Setting::Firewall { enabled: true },
        },
        CatalogEntry {
            kind: "disk_encryption",
            title: "Disk encryption",
            category: "Security",
            summary: "Checks that the system drive is encrypted, so a lost or stolen laptop doesn't leak data. Turning it on needs the user present, so this reports rather than forces it.",
            ad_equivalent: "BitLocker Drive Encryption",
            support: vec![
                ps(Windows, ReportOnly, "BitLocker status (manage-bde)"),
                ps(Macos, ReportOnly, "FileVault status (fdesetup)"),
                ps(Linux, ReportOnly, "LUKS on the root endpoint (lsblk)"),
            ],
            default: Setting::DiskEncryption { required: true },
        },
        CatalogEntry {
            kind: "automatic_updates",
            title: "Automatic updates",
            category: "Maintenance",
            summary: "Keeps operating-system security updates installing on their own.",
            ad_equivalent: "Configure Automatic Updates",
            support: vec![
                ps(Windows, Enforced, "Windows Update AU policy (NoAutoUpdate = 0)"),
                ps(Macos, Enforced, "softwareupdate --schedule on and automatic install preferences"),
                ps(Linux, Enforced, "unattended-upgrades (Debian/Ubuntu) or dnf-automatic (Fedora/RHEL)"),
            ],
            default: Setting::AutomaticUpdates { enabled: true },
        },
        CatalogEntry {
            kind: "login_banner",
            title: "Login notice",
            category: "Compliance",
            summary: "Shows a message before anyone signs in, for example an acceptable-use notice.",
            ad_equivalent: "Interactive logon: Message title/text for users attempting to log on",
            support: vec![
                ps(Windows, Enforced, "LegalNoticeCaption / LegalNoticeText"),
                ps(Macos, Enforced, "/Library/Security/PolicyBanner.txt"),
                ps(Linux, Enforced, "/etc/issue and /etc/issue.net"),
            ],
            default: Setting::LoginBanner { title: "Authorized use only".into(), text: "This computer belongs to our organization. Activity may be monitored.".into() },
        },
        CatalogEntry {
            kind: "install_packages",
            title: "Required software",
            category: "Software",
            summary: "Installs these apps if they're missing, using each OS's package manager. Package names differ per OS, so list each one you need.",
            ad_equivalent: "Software Installation (GPO) / Configuration Manager",
            support: vec![
                ps(Windows, Enforced, "winget"),
                ps(Macos, Enforced, "Homebrew (if installed)"),
                ps(Linux, Enforced, "apt, dnf, zypper or pacman"),
            ],
            default: Setting::InstallPackages { packages: vec![] },
        },
    ]
}

/// A setting as it ends up on one endpoint, with where it came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveSetting {
    pub setting: Setting,
    pub policy_id: Uuid,
    pub policy_name: String,
    /// Policies that also set this but lost on priority.
    pub overridden: Vec<String>,
}

/// Does this policy target an endpoint in `endpoint_ou`? `ou_ancestry` maps an OU
/// to its parent so links on a parent OU apply to children, like GPO
/// inheritance.
pub fn policy_applies(policy: &Policy, endpoint_ou: Option<Uuid>, ou_parent: &HashMap<Uuid, Option<Uuid>>) -> bool {
    if !policy.enabled {
        return false;
    }
    if policy.targets.all_endpoints {
        return true;
    }
    let mut cur = endpoint_ou;
    let mut guard = 0;
    while let Some(ou) = cur {
        if policy.targets.ou_ids.contains(&ou) {
            return true;
        }
        cur = ou_parent.get(&ou).copied().flatten();
        guard += 1;
        if guard > 64 {
            break;
        }
    }
    false
}

/// Resultant Set of Policy: merge every policy that applies to an endpoint.
/// Lower `priority` wins; accumulating settings (accounts, packages) add up.
pub fn effective_settings(policies: &[Policy], endpoint_ou: Option<Uuid>, ou_parent: &HashMap<Uuid, Option<Uuid>>) -> Vec<EffectiveSetting> {
    let mut applicable: Vec<&Policy> = policies.iter().filter(|p| policy_applies(p, endpoint_ou, ou_parent)).collect();
    applicable.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

    let mut out: Vec<EffectiveSetting> = Vec::new();
    let mut seen: HashMap<&'static str, usize> = HashMap::new();
    for p in applicable {
        for s in &p.settings {
            if s.accumulates() {
                out.push(EffectiveSetting { setting: s.clone(), policy_id: p.id, policy_name: p.name.clone(), overridden: vec![] });
                continue;
            }
            match seen.get(s.kind()) {
                Some(&idx) => out[idx].overridden.push(p.name.clone()),
                None => {
                    seen.insert(s.kind(), out.len());
                    out.push(EffectiveSetting { setting: s.clone(), policy_id: p.id, policy_name: p.name.clone(), overridden: vec![] });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pol(name: &str, priority: i32, targets: PolicyTargets, settings: Vec<Setting>) -> Policy {
        Policy {
            id: Uuid::new_v4(),
            name: name.into(),
            description: String::new(),
            enabled: true,
            priority,
            targets,
            settings,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn lower_priority_number_wins_and_lists_overrides() {
        let all = PolicyTargets { all_endpoints: true, ou_ids: vec![] };
        let a = pol("Baseline", 100, all.clone(), vec![Setting::ScreenLock { minutes: 15 }]);
        let b = pol("Finance", 10, all, vec![Setting::ScreenLock { minutes: 5 }]);
        let eff = effective_settings(&[a, b], None, &HashMap::new());
        assert_eq!(eff.len(), 1);
        assert_eq!(eff[0].setting, Setting::ScreenLock { minutes: 5 });
        assert_eq!(eff[0].policy_name, "Finance");
        assert_eq!(eff[0].overridden, vec!["Baseline".to_string()]);
    }

    #[test]
    fn ou_links_inherit_to_children() {
        let parent = Uuid::new_v4();
        let child = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut tree = HashMap::new();
        tree.insert(parent, None);
        tree.insert(child, Some(parent));
        tree.insert(other, None);
        let p = pol("Parent link", 1, PolicyTargets { all_endpoints: false, ou_ids: vec![parent] }, vec![Setting::Firewall { enabled: true }]);
        assert!(policy_applies(&p, Some(child), &tree));
        assert!(policy_applies(&p, Some(parent), &tree));
        assert!(!policy_applies(&p, Some(other), &tree));
        assert!(!policy_applies(&p, None, &tree));
    }

    #[test]
    fn accounts_accumulate() {
        let all = PolicyTargets { all_endpoints: true, ou_ids: vec![] };
        let g1 = Uuid::new_v4();
        let g2 = Uuid::new_v4();
        let a = pol("A", 1, all.clone(), vec![Setting::LocalAccounts { group_id: g1, admin: false }]);
        let b = pol("B", 2, all, vec![Setting::LocalAccounts { group_id: g2, admin: true }]);
        let eff = effective_settings(&[a, b], None, &HashMap::new());
        assert_eq!(eff.len(), 2);
    }

    #[test]
    fn disabled_policies_are_ignored() {
        let mut p = pol("Off", 1, PolicyTargets { all_endpoints: true, ou_ids: vec![] }, vec![Setting::Firewall { enabled: true }]);
        p.enabled = false;
        assert!(effective_settings(&[p], None, &HashMap::new()).is_empty());
    }

    #[test]
    fn catalog_covers_every_setting_kind_on_every_os() {
        let cat = catalog();
        for e in &cat {
            assert_eq!(e.kind, e.default.kind());
            assert_eq!(e.support.len(), 3, "{} must list all OSes", e.kind);
        }
    }

    #[test]
    fn setting_json_shape_is_stable() {
        let s = Setting::ScreenLock { minutes: 10 };
        assert_eq!(serde_json::to_string(&s).unwrap(), r#"{"kind":"screen_lock","minutes":10}"#);
    }
}
