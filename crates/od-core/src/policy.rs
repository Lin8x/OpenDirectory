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
    /// Lock local accounts after repeated wrong passwords.
    AccountLockout { threshold: u32, duration_minutes: u32 },
    /// Require a new password after this many days (0 = never expires).
    PasswordMaxAge { days: u32 },
    /// Remember this many old passwords so they can't be reused.
    PasswordHistory { count: u32 },
    /// Require a mix of character types.
    PasswordComplexity { required: bool },
    /// USB mass storage: "allow", "read_only" or "block".
    UsbStorage { mode: String },
    /// Built-in Guest account.
    GuestAccount { enabled: bool },
    /// Remote Desktop / Screen Sharing.
    RemoteDesktop { enabled: bool },
    /// SSH server.
    SshServer { enabled: bool },
    /// Signing in automatically without a password.
    AutoLogin { allowed: bool },
    /// NTP server for the system clock.
    TimeServer { server: String },
    /// Don't show the last signed-in user name at the sign-in screen.
    HideLastUsername { enabled: bool },
    /// Turn off AutoRun/AutoPlay for removable media.
    DisableAutorun { enabled: bool },
    /// SMB version 1 protocol.
    Smb1 { enabled: bool },
    /// Link-Local Multicast Name Resolution.
    Llmnr { enabled: bool },
    /// Real-time antivirus protection.
    Antivirus { required: bool },
    /// SmartScreen (Windows) / Gatekeeper (macOS): block untrusted apps.
    AppReputation { enabled: bool },
    /// Record sign-in and account events in the OS security log.
    SecurityAuditing { enabled: bool },
    /// Delay updates for a pilot/broad ring (patch rings).
    UpdateDeferral { quality_days: u32, feature_days: u32 },
    /// Run a library script at startup, at sign-in, on a schedule or once
    /// (GPO Scripts). Parameter values are passed as environment variables.
    RunScript {
        script_id: Uuid,
        trigger: crate::scripts::ScriptTrigger,
        #[serde(default)]
        params: std::collections::BTreeMap<String, String>,
    },
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
            Setting::RunScript { .. } => "run_script",
            Setting::AccountLockout { .. } => "account_lockout",
            Setting::PasswordMaxAge { .. } => "password_max_age",
            Setting::PasswordHistory { .. } => "password_history",
            Setting::PasswordComplexity { .. } => "password_complexity",
            Setting::UsbStorage { .. } => "usb_storage",
            Setting::GuestAccount { .. } => "guest_account",
            Setting::RemoteDesktop { .. } => "remote_desktop",
            Setting::SshServer { .. } => "ssh_server",
            Setting::AutoLogin { .. } => "auto_login",
            Setting::TimeServer { .. } => "time_server",
            Setting::HideLastUsername { .. } => "hide_last_username",
            Setting::DisableAutorun { .. } => "disable_autorun",
            Setting::Smb1 { .. } => "smb1",
            Setting::Llmnr { .. } => "llmnr",
            Setting::Antivirus { .. } => "antivirus",
            Setting::AppReputation { .. } => "app_reputation",
            Setting::SecurityAuditing { .. } => "security_auditing",
            Setting::UpdateDeferral { .. } => "update_deferral",
        }
    }

    /// Settings of these kinds add up across policies instead of the
    /// highest-priority one winning.
    pub fn accumulates(&self) -> bool {
        matches!(self, Setting::LocalAccounts { .. } | Setting::InstallPackages { .. } | Setting::RunScript { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Setting::PasswordMinLength { length } => format!("Passwords at least {length} characters"),
            Setting::ScreenLock { minutes } => format!("Lock screen after {minutes} min idle"),
            Setting::Firewall { enabled } => {
                if *enabled {
                    "Firewall on".into()
                } else {
                    "Firewall not enforced".into()
                }
            }
            Setting::DiskEncryption { required } => {
                if *required {
                    "Disk encryption required".into()
                } else {
                    "Disk encryption optional".into()
                }
            }
            Setting::AutomaticUpdates { enabled } => {
                if *enabled {
                    "Automatic updates on".into()
                } else {
                    "Automatic updates not enforced".into()
                }
            }
            Setting::LocalAccounts { admin, .. } => {
                if *admin {
                    "Sign-in accounts for a group (as admins)".into()
                } else {
                    "Sign-in accounts for a group".into()
                }
            }
            Setting::LoginBanner { title, .. } => format!("Login notice: {title}"),
            Setting::InstallPackages { packages } => format!("Install {}", packages.join(", ")),
            Setting::RunScript { trigger, .. } => format!("Run a script {}", trigger.describe()),
            Setting::AccountLockout { threshold, duration_minutes } => {
                format!("Lock accounts for {duration_minutes} min after {threshold} wrong passwords")
            }
            Setting::PasswordMaxAge { days: 0 } => "Passwords never expire".into(),
            Setting::PasswordMaxAge { days } => format!("Passwords expire after {days} days"),
            Setting::PasswordHistory { count } => format!("Remember {count} previous passwords"),
            Setting::PasswordComplexity { required } => onoff(*required, "Complex passwords required", "Password complexity not enforced"),
            Setting::UsbStorage { mode } => match mode.as_str() {
                "block" => "USB storage blocked".into(),
                "read_only" => "USB storage read-only".into(),
                _ => "USB storage allowed".into(),
            },
            Setting::GuestAccount { enabled } => onoff(*enabled, "Guest account on", "Guest account off"),
            Setting::RemoteDesktop { enabled } => onoff(*enabled, "Remote Desktop on", "Remote Desktop off"),
            Setting::SshServer { enabled } => onoff(*enabled, "SSH server on", "SSH server off"),
            Setting::AutoLogin { allowed } => onoff(*allowed, "Automatic sign-in allowed", "Automatic sign-in off"),
            Setting::TimeServer { server } => format!("Time from {server}"),
            Setting::HideLastUsername { enabled } => onoff(*enabled, "Last user name hidden", "Last user name shown"),
            Setting::DisableAutorun { enabled } => onoff(*enabled, "AutoRun off", "AutoRun not enforced"),
            Setting::Smb1 { enabled } => onoff(*enabled, "SMBv1 on", "SMBv1 off"),
            Setting::Llmnr { enabled } => onoff(*enabled, "LLMNR on", "LLMNR off"),
            Setting::Antivirus { required } => onoff(*required, "Real-time antivirus required", "Antivirus not enforced"),
            Setting::AppReputation { enabled } => onoff(*enabled, "Untrusted apps blocked", "App reputation not enforced"),
            Setting::SecurityAuditing { enabled } => onoff(*enabled, "Security auditing on", "Security auditing not enforced"),
            Setting::UpdateDeferral { quality_days, feature_days } => {
                format!("Defer quality updates {quality_days} d, feature updates {feature_days} d")
            }
        }
    }
}

fn onoff(on: bool, yes: &str, no: &str) -> String {
    if on { yes } else { no }.to_string()
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
    /// Allowed values for text fields that are really a choice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<FieldChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldChoice {
    pub field: &'static str,
    /// (value, label)
    pub options: Vec<(&'static str, &'static str)>,
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
                ps(Macos, Enforced, "Local accounts via dscl and dseditgroup"),
                ps(Linux, Enforced, "Local accounts via useradd, chpasswd and the sudo/wheel group"),
            ],
            choices: vec![],
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
            choices: vec![],
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
                ps(Macos, Enforced, "Screen saver idle time with password on wake"),
                ps(Linux, Enforced, "GNOME dconf system defaults (idle-delay, lock-enabled)"),
            ],
            choices: vec![],
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
            choices: vec![],
            default: Setting::Firewall { enabled: true },
        },
        CatalogEntry {
            kind: "disk_encryption",
            title: "Disk encryption",
            category: "Security",
            summary: "Checks that the system drive is encrypted, so a lost or stolen laptop doesn't leak data. Turning it on needs the user present, so this reports rather than forces it.",
            ad_equivalent: "BitLocker Drive Encryption",
            support: vec![
                ps(Windows, ReportOnly, "BitLocker status (Get-BitLockerVolume)"),
                ps(Macos, ReportOnly, "FileVault status (fdesetup)"),
                ps(Linux, ReportOnly, "LUKS on the root filesystem (lsblk)"),
            ],
            choices: vec![],
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
                ps(Macos, Enforced, "Software Update automatic check, download and install preferences"),
                ps(Linux, Enforced, "unattended-upgrades (Debian/Ubuntu) or dnf-automatic (Fedora/RHEL)"),
            ],
            choices: vec![],
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
            choices: vec![],
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
            choices: vec![],
            default: Setting::InstallPackages { packages: vec![] },
        },
        CatalogEntry {
            kind: "run_script",
            title: "Scripts (startup, sign-in, scheduled)",
            category: "Scripts",
            summary: "Runs a script from your Script Library on every endpoint this GPO applies to: when the computer starts, when someone signs in, on a schedule, or once.",
            ad_equivalent: "Computer/User Configuration > Windows Settings > Scripts",
            support: vec![
                ps(Windows, Enforced, "Endpoint Agent runs it as SYSTEM"),
                ps(Macos, Enforced, "Endpoint Agent runs it as root"),
                ps(Linux, Enforced, "Endpoint Agent runs it as root"),
            ],
            choices: vec![],
            default: Setting::RunScript { script_id: Uuid::nil(), trigger: crate::scripts::ScriptTrigger::Startup, params: Default::default() },
        },
        e(
            "account_lockout", "Account lockout", "Accounts",
            "Locks a local account for a while after too many wrong passwords, which stops password-guessing attacks.",
            "Account Lockout Policy: threshold and duration",
            [(Enforced, "net accounts /lockoutthreshold /lockoutduration"), (Enforced, "pwpolicy maxFailedLoginAttempts and minutesUntilFailedLoginReset"), (Enforced, "pam_faillock deny and unlock_time (faillock.conf)")],
            Setting::AccountLockout { threshold: 5, duration_minutes: 15 },
        ),
        e(
            "password_max_age", "Maximum password age", "Accounts",
            "Makes people choose a new password after this many days. Current guidance (NIST) prefers long passwords that don't expire unless there's a breach, so 0 (never) is a valid choice.",
            "Password Policy: Maximum password age",
            [(Enforced, "net accounts /maxpwage"), (Enforced, "pwpolicy maxMinutesUntilChangePassword"), (Enforced, "PASS_MAX_DAYS in /etc/login.defs and chage for managed accounts")],
            Setting::PasswordMaxAge { days: 0 },
        ),
        e(
            "password_history", "Password history", "Accounts",
            "Remembers previous passwords so people can't switch back to an old one.",
            "Password Policy: Enforce password history",
            [(Enforced, "net accounts /uniquepw"), (Enforced, "pwpolicy usingHistory"), (Enforced, "pam_pwhistory remember (pwhistory.conf)")],
            Setting::PasswordHistory { count: 10 },
        ),
        e(
            "password_complexity", "Password complexity", "Accounts",
            "Requires passwords to mix upper-case, lower-case, digits and symbols. Length matters more, so pair this with a minimum length.",
            "Password must meet complexity requirements",
            [(Enforced, "Local security policy (secedit) PasswordComplexity"), (Enforced, "pwpolicy requiresAlpha and requiresNumeric"), (Enforced, "pam_pwquality minclass = 3")],
            Setting::PasswordComplexity { required: true },
        ),
        CatalogEntry {
            kind: "usb_storage",
            title: "USB storage",
            category: "Devices",
            summary: "Controls USB drives and memory sticks. Read-only lets people open files but not copy data out; block stops USB storage entirely. Keyboards and mice keep working.",
            ad_equivalent: "Removable Storage Access / Removable Disks: Deny write access",
            support: vec![
                ps(Windows, Enforced, "RemovableStorageDevices policy (Deny_Write / Deny_All)"),
                ps(Macos, ReportOnly, "Needs an MDM configuration profile; reports only"),
                ps(Linux, Enforced, "usb-storage kernel module blocked via modprobe.d (block); read-only via udev rule"),
            ],
            default: Setting::UsbStorage { mode: "read_only".into() },
            choices: vec![FieldChoice { field: "mode", options: vec![("allow", "Allow"), ("read_only", "Read-only"), ("block", "Block")] }],
        },
        e(
            "guest_account", "Guest account", "Accounts",
            "The built-in Guest account lets anyone use the computer without a password. Turn it off on managed computers.",
            "Accounts: Guest account status",
            [(Enforced, "net user Guest /active:no"), (Enforced, "com.apple.loginwindow GuestEnabled"), (Enforced, "LightDM allow-guest / GDM (no guest by default)")],
            Setting::GuestAccount { enabled: false },
        ),
        e(
            "remote_desktop", "Remote Desktop", "Remote access",
            "Lets people connect to this computer's screen from elsewhere. Turn off unless your IT team uses it.",
            "Allow users to connect remotely by using Remote Desktop Services",
            [(Enforced, "fDenyTSConnections and the Remote Desktop firewall group"), (Enforced, "Screen Sharing launch daemon"), (Enforced, "gnome-remote-desktop / xrdp service")],
            Setting::RemoteDesktop { enabled: false },
        ),
        e(
            "ssh_server", "SSH server", "Remote access",
            "Allows command-line sign-in over the network with SSH. Usually on for servers and off for laptops.",
            "OpenSSH Server (Windows optional feature)",
            [(Enforced, "sshd service (OpenSSH Server)"), (Enforced, "Remote Login (systemsetup -setremotelogin)"), (Enforced, "sshd / ssh systemd service")],
            Setting::SshServer { enabled: false },
        ),
        e(
            "auto_login", "Automatic sign-in", "Accounts",
            "Automatic sign-in skips the password at startup, so anyone who turns the computer on is signed in. Turn it off.",
            "AutoAdminLogon",
            [(Enforced, "Winlogon AutoAdminLogon = 0"), (Enforced, "Removes loginwindow autoLoginUser"), (Enforced, "GDM AutomaticLoginEnable / LightDM autologin-user")],
            Setting::AutoLogin { allowed: false },
        ),
        e(
            "time_server", "Time server (NTP)", "System",
            "Keeps the clock in sync with a time server. Correct time matters for sign-in (Kerberos), logs and certificates.",
            "Configure Windows NTP Client: NtpServer",
            [(Enforced, "w32tm /config /manualpeerlist"), (Enforced, "systemsetup -setnetworktimeserver"), (Enforced, "systemd-timesyncd NTP= or chrony")],
            Setting::TimeServer { server: "pool.ntp.org".into() },
        ),
        e(
            "hide_last_username", "Hide last user name", "Security",
            "The sign-in screen asks for a user name instead of showing who signed in last, so attackers learn less.",
            "Interactive logon: Don't display last signed-in",
            [(Enforced, "DontDisplayLastUserName"), (Enforced, "loginwindow SHOWFULLNAME (name and password fields)"), (Enforced, "GDM disable-user-list")],
            Setting::HideLastUsername { enabled: true },
        ),
        e(
            "disable_autorun", "Turn off AutoRun", "Devices",
            "Stops programs on USB drives and discs from starting by themselves when inserted, a classic way malware spreads.",
            "Turn off AutoPlay / NoDriveTypeAutoRun",
            [(Enforced, "NoDriveTypeAutoRun = 255, NoAutorun = 1"), (Enforced, "Not applicable: macOS never auto-runs media"), (Enforced, "GNOME autorun-never")],
            Setting::DisableAutorun { enabled: true },
        ),
        e(
            "smb1", "SMBv1 protocol", "Network",
            "SMB version 1 is an old, insecure file-sharing protocol (WannaCry spread through it). Keep it off.",
            "SMB 1.0/CIFS File Sharing Support",
            [(Enforced, "Set-SmbServerConfiguration -EnableSMB1Protocol and the SMB1 optional feature"), (Enforced, "Not applicable: macOS doesn't serve SMBv1"), (Enforced, "Samba server min protocol = SMB2 (if Samba is installed)")],
            Setting::Smb1 { enabled: false },
        ),
        e(
            "llmnr", "LLMNR name resolution", "Network",
            "LLMNR lets computers answer name lookups on the local network, which attackers abuse to capture passwords. Turn it off when you have DNS.",
            "Turn off multicast name resolution",
            [(Enforced, "DNSClient EnableMulticast = 0"), (Enforced, "Not applicable: macOS doesn't use LLMNR"), (Enforced, "systemd-resolved LLMNR=no")],
            Setting::Llmnr { enabled: false },
        ),
        e(
            "antivirus", "Real-time antivirus", "Security",
            "Requires real-time malware protection. Windows uses Microsoft Defender; macOS has XProtect built in; on Linux it reports whether ClamAV's on-access scanner runs.",
            "Microsoft Defender Antivirus: Turn on real-time protection",
            [(Enforced, "Set-MpPreference -DisableRealtimeMonitoring $false"), (ReportOnly, "XProtect (always on); reports its status"), (ReportOnly, "Reports clamav-daemon / clamonacc status")],
            Setting::Antivirus { required: true },
        ),
        e(
            "app_reputation", "Block untrusted apps", "Security",
            "Warns about or blocks apps and downloads that aren't from identified developers.",
            "Windows Defender SmartScreen",
            [(Enforced, "SmartScreen EnableSmartScreen = 1"), (Enforced, "Gatekeeper (spctl --global-enable)"), (Unsupported, "No built-in equivalent")],
            Setting::AppReputation { enabled: true },
        ),
        e(
            "security_auditing", "Security auditing", "Security",
            "Records sign-ins, failed sign-ins and account changes in the computer's security log, so incidents can be investigated.",
            "Advanced Audit Policy: Logon/Logoff and Account Management",
            [(Enforced, "auditpol success and failure for logon and account management"), (ReportOnly, "Unified log records sign-ins by default"), (Enforced, "auditd service enabled (if installed)")],
            Setting::SecurityAuditing { enabled: true },
        ),
        e(
            "update_deferral", "Update deferral (patch rings)", "Updates",
            "Delays updates so a pilot group gets them first. Use a GPO with 0 days for the pilot OU and 7 or more for everyone else.",
            "Windows Update for Business: Select when Quality/Feature Updates are received",
            [(Enforced, "DeferQualityUpdatesPeriodInDays / DeferFeatureUpdatesPeriodInDays"), (ReportOnly, "Needs an MDM profile (enforcedSoftwareUpdateDelay); reports only"), (Unsupported, "Use a scheduled script from the Script Library")],
            Setting::UpdateDeferral { quality_days: 7, feature_days: 30 },
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn e(
    kind: &'static str,
    title: &'static str,
    category: &'static str,
    summary: &'static str,
    ad_equivalent: &'static str,
    support: [(SupportLevel, &'static str); 3],
    default: Setting,
) -> CatalogEntry {
    let [w, m, l] = support;
    CatalogEntry {
        kind,
        title,
        category,
        summary,
        ad_equivalent,
        support: vec![ps(OsFamily::Windows, w.0, w.1), ps(OsFamily::Macos, m.0, m.1), ps(OsFamily::Linux, l.0, l.1)],
        default,
        choices: vec![],
    }
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
pub fn effective_settings(
    policies: &[Policy],
    endpoint_ou: Option<Uuid>,
    ou_parent: &HashMap<Uuid, Option<Uuid>>,
) -> Vec<EffectiveSetting> {
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
        let p =
            pol("Parent link", 1, PolicyTargets { all_endpoints: false, ou_ids: vec![parent] }, vec![Setting::Firewall { enabled: true }]);
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
