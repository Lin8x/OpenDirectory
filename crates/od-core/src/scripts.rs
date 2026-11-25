//! The script library: saved, parameterized scripts an admin can run on
//! demand against endpoints, security groups or OUs, or attach to a GPO as a
//! startup, sign-in or scheduled script (like GPO Scripts in Active Directory).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{OsFamily, Shell};

/// A named input for a script. The agent passes each one to the script as an
/// environment variable `OD_PARAM_<NAME>`, never by pasting text into the
/// script, so values can't change what the script does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptParam {
    /// Upper-case letters, digits and underscores, e.g. `USERNAME`.
    pub name: String,
    pub label: String,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub required: bool,
    /// Hidden in run history and the Admin Center (passwords, tokens).
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub category: String,
    pub shell: Shell,
    pub body: String,
    /// Operating systems the script is written for. Empty means any.
    pub os: Vec<OsFamily>,
    pub params: Vec<ScriptParam>,
    /// Template this script was created from, if any.
    pub template: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Script {
    pub fn supports(&self, os: OsFamily) -> bool {
        self.os.is_empty() || self.os.contains(&os)
    }
}

/// Where an on-demand run goes. Everything listed is combined and de-duplicated.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunTargets {
    #[serde(default)]
    pub all_endpoints: bool,
    #[serde(default)]
    pub endpoint_ids: Vec<Uuid>,
    /// Security groups: their computer members.
    #[serde(default)]
    pub group_ids: Vec<Uuid>,
    /// OUs, including child OUs.
    #[serde(default)]
    pub ou_ids: Vec<Uuid>,
}

impl RunTargets {
    pub fn is_empty(&self) -> bool {
        !self.all_endpoints && self.endpoint_ids.is_empty() && self.group_ids.is_empty() && self.ou_ids.is_empty()
    }
}

/// When a GPO script runs on an endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScriptTrigger {
    /// Once each time the computer starts (as SYSTEM/root).
    Startup,
    /// Each time someone signs in. Runs as SYSTEM/root with `OD_LOGON_USER` set.
    Logon,
    /// Repeatedly, every N minutes.
    Interval { minutes: u32 },
    /// Once per endpoint, and again whenever the script changes.
    Once,
}

impl ScriptTrigger {
    pub fn describe(&self) -> String {
        match self {
            ScriptTrigger::Startup => "at startup".into(),
            ScriptTrigger::Logon => "at sign-in".into(),
            ScriptTrigger::Interval { minutes } if minutes % 1440 == 0 => format!("every {} day(s)", minutes / 1440),
            ScriptTrigger::Interval { minutes } if minutes % 60 == 0 => format!("every {} hour(s)", minutes / 60),
            ScriptTrigger::Interval { minutes } => format!("every {minutes} min"),
            ScriptTrigger::Once => "once".into(),
        }
    }
}

/// A script resolved for one endpoint, sent with check-in.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyScript {
    pub script_id: Uuid,
    pub name: String,
    /// Changes whenever the script body, parameters or trigger change.
    pub revision: String,
    pub shell: Shell,
    pub body: String,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub trigger: ScriptTrigger,
    /// Shown in the job history, e.g. `GPO "Default Endpoint Policy"`.
    pub source: String,
}

/// What an agent reports after running a GPO script.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptRunReport {
    pub script_id: Uuid,
    pub trigger: ScriptTrigger,
    pub source: String,
    pub shell: Shell,
    pub body: String,
    pub exit_code: Option<i32>,
    pub output: String,
    pub success: bool,
    pub started_at: DateTime<Utc>,
}

pub fn valid_param_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 40
        && n.starts_with(|c: char| c.is_ascii_uppercase())
        && n.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Environment for a run: `OD_PARAM_<NAME>` for each parameter, using the
/// supplied value or the default.
pub fn param_env(params: &[ScriptParam], values: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>, String> {
    let mut env = BTreeMap::new();
    for p in params {
        let v = values.get(&p.name).cloned().unwrap_or_else(|| p.default.clone());
        if p.required && v.trim().is_empty() {
            return Err(format!("\"{}\" is required", p.label));
        }
        if v.contains('\0') || v.len() > 4096 {
            return Err(format!("\"{}\" has an invalid value", p.label));
        }
        env.insert(format!("OD_PARAM_{}", p.name), v);
    }
    Ok(env)
}

/// A built-in starting point for the script library.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptTemplate {
    pub key: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub os: Vec<OsFamily>,
    pub shell: Shell,
    pub body: &'static str,
    pub params: Vec<ScriptParam>,
}

fn param(name: &str, label: &str, default: &str, required: bool) -> ScriptParam {
    ScriptParam { name: name.into(), label: label.into(), default: default.into(), required, secret: false }
}
fn secret(name: &str, label: &str) -> ScriptParam {
    ScriptParam { name: name.into(), label: label.into(), default: String::new(), required: true, secret: true }
}

/// Does this environment variable carry a secret? Used to hide values.
pub fn is_secret_env(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["PASSWORD", "SECRET", "TOKEN", "PASSPHRASE"].iter().any(|w| n.contains(w))
}

/// Common sysadmin tasks for each OS, ready to copy into the library.
pub fn templates() -> Vec<ScriptTemplate> {
    use OsFamily::*;
    vec![
        // ---------------------------------------------------------- Windows
        ScriptTemplate {
            key: "win-disk-cleanup",
            name: "Clean up temporary files",
            category: "Maintenance",
            description: "Deletes files older than N days from the Windows and user temp folders and empties the Recycle Bin.",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: r#"$days = [int]($env:OD_PARAM_DAYS)
$cut = (Get-Date).AddDays(-$days)
$paths = @("$env:WINDIR\Temp") + (Get-ChildItem C:\Users -Directory | ForEach-Object { Join-Path $_.FullName 'AppData\Local\Temp' })
$freed = 0
foreach ($p in $paths) {
  if (Test-Path $p) {
    Get-ChildItem $p -Recurse -Force -ErrorAction SilentlyContinue | Where-Object { -not $_.PSIsContainer -and $_.LastWriteTime -lt $cut } | ForEach-Object {
      $freed += $_.Length; Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue
    }
  }
}
Clear-RecycleBin -Force -ErrorAction SilentlyContinue
"Freed {0:N1} MB" -f ($freed / 1MB)"#,
            params: vec![param("DAYS", "Delete files older than (days)", "7", true)],
        },
        ScriptTemplate {
            key: "win-flush-dns",
            name: "Flush DNS and renew IP",
            category: "Network",
            description: "Clears the DNS resolver cache and renews the DHCP lease on every adapter.",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: "Clear-DnsClientCache\nipconfig /release | Out-Null\nipconfig /renew | Out-Null\nGet-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.IPAddress -notlike '127.*' } | Format-Table InterfaceAlias, IPAddress -AutoSize",
            params: vec![],
        },
        ScriptTemplate {
            key: "win-installed-software",
            name: "List installed software",
            category: "Inventory",
            description: "Lists installed applications with version and publisher from the registry uninstall keys.",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: r#"$keys = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
Get-ItemProperty $keys -ErrorAction SilentlyContinue | Where-Object DisplayName | Sort-Object DisplayName | Format-Table DisplayName, DisplayVersion, Publisher -AutoSize | Out-String -Width 200"#,
            params: vec![],
        },
        ScriptTemplate {
            key: "win-restart-service",
            name: "Restart a service",
            category: "Services",
            description: "Restarts a Windows service by name and shows its new status.",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: "Restart-Service -Name $env:OD_PARAM_SERVICE -Force -ErrorAction Stop\nGet-Service -Name $env:OD_PARAM_SERVICE | Format-List Name, DisplayName, Status",
            params: vec![param("SERVICE", "Service name", "Spooler", true)],
        },
        ScriptTemplate {
            key: "win-gpupdate-wu",
            name: "Check for Windows updates",
            category: "Updates",
            description: "Starts a Windows Update scan and lists pending updates using the built-in Update Session API.",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: r#"$s = New-Object -ComObject Microsoft.Update.Session
$r = $s.CreateUpdateSearcher().Search("IsInstalled=0 and IsHidden=0")
if ($r.Updates.Count -eq 0) { "No pending updates" } else { $r.Updates | ForEach-Object { "{0}  ({1} MB)" -f $_.Title, [math]::Round($_.MaxDownloadSize/1MB) } }"#,
            params: vec![],
        },
        ScriptTemplate {
            key: "win-bitlocker-status",
            name: "BitLocker status",
            category: "Security",
            description: "Shows BitLocker protection, encryption percentage and key protectors for every volume.",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: "Get-BitLockerVolume | Format-Table MountPoint, VolumeStatus, ProtectionStatus, EncryptionPercentage, KeyProtector -AutoSize | Out-String -Width 200",
            params: vec![],
        },
        ScriptTemplate {
            key: "win-local-admins",
            name: "List local administrators",
            category: "Security",
            description: "Lists every member of the local Administrators group, to spot unexpected admin rights.",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: "Get-LocalGroupMember -SID S-1-5-32-544 | Format-Table Name, ObjectClass, PrincipalSource -AutoSize",
            params: vec![],
        },
        ScriptTemplate {
            key: "win-event-errors",
            name: "Recent system errors",
            category: "Troubleshooting",
            description: "Shows the newest error and critical events from the System log.",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: "Get-WinEvent -FilterHashtable @{LogName='System'; Level=1,2} -MaxEvents ([int]$env:OD_PARAM_COUNT) | Format-Table TimeCreated, ProviderName, Id, Message -Wrap | Out-String -Width 200",
            params: vec![param("COUNT", "Number of events", "25", true)],
        },
        ScriptTemplate {
            key: "win-map-drive",
            name: "Map a network drive (sign-in script)",
            category: "Files",
            description: "Maps a drive letter to an SMB share for the signed-in user. Attach it to a GPO as a sign-in script.",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: r#"$letter = $env:OD_PARAM_LETTER.TrimEnd(':')
$share = $env:OD_PARAM_SHARE
if (-not $env:OD_LOGON_USER) { "Run this as a sign-in script so it knows who signed in."; exit 0 }
$cmd = "net use ${letter}: `"$share`" /persistent:yes"
schtasks /Create /TN "OpenDirectory\MapDrive-$letter" /TR "cmd /c $cmd" /SC ONLOGON /RU $env:OD_LOGON_USER /F | Out-Null
schtasks /Run /TN "OpenDirectory\MapDrive-$letter" | Out-Null
"Mapped ${letter}: to $share for $env:OD_LOGON_USER""#,
            params: vec![param("LETTER", "Drive letter", "S", true), param("SHARE", "Share path", r"\\fileserver\shared", true)],
        },
        // ---------------------------------------------------------- macOS
        ScriptTemplate {
            key: "mac-software-updates",
            name: "List and install macOS updates",
            category: "Updates",
            description: "Lists available software updates, and installs them when INSTALL is yes.",
            os: vec![Macos],
            shell: Shell::Zsh,
            body: "softwareupdate --list 2>&1\nif [ \"$OD_PARAM_INSTALL\" = \"yes\" ]; then softwareupdate --install --all --agree-to-license 2>&1; fi",
            params: vec![param("INSTALL", "Install now (yes/no)", "no", false)],
        },
        ScriptTemplate {
            key: "mac-filevault-status",
            name: "FileVault status",
            category: "Security",
            description: "Shows whether FileVault disk encryption is on and which users can unlock the disk.",
            os: vec![Macos],
            shell: Shell::Zsh,
            body: "fdesetup status\necho\necho 'Users who can unlock:'\nfdesetup list 2>/dev/null || echo '(needs FileVault on)'",
            params: vec![],
        },
        ScriptTemplate {
            key: "mac-flush-dns",
            name: "Flush DNS cache",
            category: "Network",
            description: "Clears the macOS DNS cache and restarts mDNSResponder.",
            os: vec![Macos],
            shell: Shell::Zsh,
            body: "dscacheutil -flushcache && killall -HUP mDNSResponder && echo 'DNS cache flushed'",
            params: vec![],
        },
        ScriptTemplate {
            key: "mac-installed-apps",
            name: "List installed apps",
            category: "Inventory",
            description: "Lists apps in /Applications with their version.",
            os: vec![Macos],
            shell: Shell::Zsh,
            body: "for a in /Applications/*.app; do v=$(defaults read \"$a/Contents/Info\" CFBundleShortVersionString 2>/dev/null); printf '%-50s %s\\n' \"$(basename \"$a\" .app)\" \"$v\"; done",
            params: vec![],
        },
        // ---------------------------------------------------------- Linux
        ScriptTemplate {
            key: "linux-upgrade",
            name: "Install OS updates",
            category: "Updates",
            description: "Refreshes package lists and installs all updates with apt, dnf, zypper or pacman.",
            os: vec![Linux],
            shell: Shell::Bash,
            body: r#"set -e
if command -v apt-get >/dev/null; then export DEBIAN_FRONTEND=noninteractive; apt-get update -q && apt-get -yq upgrade
elif command -v dnf >/dev/null; then dnf -y upgrade
elif command -v zypper >/dev/null; then zypper -n update
elif command -v pacman >/dev/null; then pacman -Syu --noconfirm
else echo "No supported package manager found"; exit 1; fi
[ -f /var/run/reboot-required ] && echo "A restart is needed to finish updating." || true"#,
            params: vec![],
        },
        ScriptTemplate {
            key: "linux-disk-usage",
            name: "Disk usage report",
            category: "Troubleshooting",
            description: "Shows free space per filesystem and the largest folders under a path.",
            os: vec![Linux, Macos],
            shell: Shell::Bash,
            body: "df -h\necho\necho \"Largest folders in $OD_PARAM_PATH:\"\ndu -xh \"$OD_PARAM_PATH\" 2>/dev/null | sort -rh | head -n 15",
            params: vec![param("PATH", "Folder to inspect", "/var", true)],
        },
        ScriptTemplate {
            key: "linux-restart-service",
            name: "Restart a systemd service",
            category: "Services",
            description: "Restarts a systemd unit and shows its status.",
            os: vec![Linux],
            shell: Shell::Bash,
            body: "systemctl restart \"$OD_PARAM_SERVICE\" && systemctl --no-pager status \"$OD_PARAM_SERVICE\" | head -n 15",
            params: vec![param("SERVICE", "Service name", "cups", true)],
        },
        ScriptTemplate {
            key: "linux-failed-logins",
            name: "Recent failed sign-ins",
            category: "Security",
            description: "Lists recent failed SSH and console sign-in attempts from the journal.",
            os: vec![Linux],
            shell: Shell::Bash,
            body: "journalctl --no-pager -q --since \"-${OD_PARAM_HOURS}h\" | grep -Ei 'failed password|authentication failure|invalid user' | tail -n 50 || echo 'None found'",
            params: vec![param("HOURS", "Look back (hours)", "24", true)],
        },
        // ---------------------------------------------------------- domain join
        ScriptTemplate {
            key: "win-domain-join",
            name: "Join Windows to the domain",
            category: "Domain",
            description: "Joins this computer to an Active Directory or Samba domain (Windows Pro, Enterprise or Education). The computer's DNS must point at a domain controller.",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: r#"$pw = ConvertTo-SecureString $env:OD_PARAM_JOIN_PASSWORD -AsPlainText -Force
$cred = New-Object System.Management.Automation.PSCredential($env:OD_PARAM_JOIN_USER, $pw)
$args = @{ DomainName = $env:OD_PARAM_DOMAIN; Credential = $cred; Force = $true }
if ($env:OD_PARAM_OU_PATH) { $args.OUPath = $env:OD_PARAM_OU_PATH }
Add-Computer @args -ErrorAction Stop
"Joined $env:OD_PARAM_DOMAIN."
if ($env:OD_PARAM_RESTART -eq 'yes') { shutdown.exe /r /t 60 /c "Your IT team joined this computer to the domain. Restarting in 1 minute."; "Restarting in 1 minute." } else { "Restart to finish joining." }"#,
            params: vec![
                param("DOMAIN", "Domain", "corp.example.com", true),
                param("JOIN_USER", "Account allowed to join computers", "CORP\\Administrator", true),
                secret("JOIN_PASSWORD", "Its password"),
                param("OU_PATH", "OU for the computer (optional)", "", false),
                param("RESTART", "Restart afterwards (yes/no)", "yes", false),
            ],
        },
        ScriptTemplate {
            key: "linux-realm-join",
            name: "Join Linux to the domain (realmd/SSSD)",
            category: "Domain",
            description: "Installs realmd and SSSD, joins an Active Directory or Samba domain, and creates home folders at first sign-in.",
            os: vec![Linux],
            shell: Shell::Bash,
            body: r#"set -e
if command -v apt-get >/dev/null; then export DEBIAN_FRONTEND=noninteractive; apt-get update -qq; apt-get install -yq realmd sssd sssd-tools adcli libnss-sss libpam-sss samba-common-bin oddjob oddjob-mkhomedir packagekit
elif command -v dnf >/dev/null; then dnf -y install realmd sssd adcli oddjob oddjob-mkhomedir samba-common-tools; fi
printf '%s' "$OD_PARAM_JOIN_PASSWORD" | realm join --user="$OD_PARAM_JOIN_USER" "$OD_PARAM_DOMAIN"
command -v pam-auth-update >/dev/null && pam-auth-update --enable mkhomedir || true
command -v authselect >/dev/null && authselect select sssd with-mkhomedir --force || true
realm list"#,
            params: vec![param("DOMAIN", "Domain", "corp.example.com", true), param("JOIN_USER", "Account allowed to join computers", "Administrator", true), secret("JOIN_PASSWORD", "Its password")],
        },
        ScriptTemplate {
            key: "mac-ad-bind",
            name: "Bind a Mac to the domain",
            category: "Domain",
            description: "Binds this Mac to an Active Directory or Samba domain with dsconfigad, so domain accounts can sign in.",
            os: vec![Macos],
            shell: Shell::Zsh,
            body: r#"dsconfigad -add "$OD_PARAM_DOMAIN" -computer "$(scutil --get LocalHostName)" -username "$OD_PARAM_JOIN_USER" -password "$OD_PARAM_JOIN_PASSWORD" -mobile enable -mobileconfirm disable -localhome enable -useuncpath enable -force
dsconfigad -show"#,
            params: vec![param("DOMAIN", "Domain", "corp.example.com", true), param("JOIN_USER", "Account allowed to join computers", "Administrator", true), secret("JOIN_PASSWORD", "Its password")],
        },
        // ---------------------------------------------------------- printing
        ScriptTemplate {
            key: "win-deploy-printer",
            name: "Deploy a printer",
            category: "Printing",
            description: "Connects a shared printer from a print server (run as a sign-in script), or adds a TCP/IP printer by address (run as a startup script).",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: r#"if ($env:OD_PARAM_SHARE) {
  Add-Printer -ConnectionName $env:OD_PARAM_SHARE -ErrorAction Stop
  if ($env:OD_PARAM_DEFAULT -eq 'yes') { (New-Object -ComObject WScript.Network).SetDefaultPrinter($env:OD_PARAM_SHARE) }
  "Connected $env:OD_PARAM_SHARE"
} else {
  $port = "IP_$env:OD_PARAM_ADDRESS"
  if (-not (Get-PrinterPort -Name $port -ErrorAction SilentlyContinue)) { Add-PrinterPort -Name $port -PrinterHostAddress $env:OD_PARAM_ADDRESS }
  if (-not (Get-Printer -Name $env:OD_PARAM_NAME -ErrorAction SilentlyContinue)) { Add-Printer -Name $env:OD_PARAM_NAME -PortName $port -DriverName $env:OD_PARAM_DRIVER }
  "Added $env:OD_PARAM_NAME at $env:OD_PARAM_ADDRESS"
}"#,
            params: vec![
                param("SHARE", "Shared printer (\\\\server\\queue), or leave empty", "", false),
                param("ADDRESS", "Or printer IP address", "", false),
                param("NAME", "Printer name (IP printers)", "Office Printer", false),
                param("DRIVER", "Driver (IP printers)", "Microsoft IPP Class Driver", false),
                param("DEFAULT", "Make it the default (yes/no)", "no", false),
            ],
        },
        ScriptTemplate {
            key: "unix-deploy-printer",
            name: "Deploy a printer (CUPS)",
            category: "Printing",
            description: "Adds a network printer with IPP Everywhere (driverless) on macOS or Linux.",
            os: vec![Macos, Linux],
            shell: Shell::Bash,
            body: r#"set -e
NAME=$(printf '%s' "$OD_PARAM_NAME" | tr -c 'A-Za-z0-9_-' '_')
lpadmin -p "$NAME" -E -v "ipp://$OD_PARAM_ADDRESS/ipp/print" -m everywhere -D "$OD_PARAM_NAME" -L "$OD_PARAM_LOCATION"
[ "$OD_PARAM_DEFAULT" = "yes" ] && lpadmin -d "$NAME"
lpstat -p "$NAME""#,
            params: vec![
                param("ADDRESS", "Printer IP address or name", "", true),
                param("NAME", "Printer name", "Office Printer", true),
                param("LOCATION", "Location", "", false),
                param("DEFAULT", "Make it the default (yes/no)", "no", false),
            ],
        },
        // ---------------------------------------------------------- any OS
        ScriptTemplate {
            key: "any-uptime",
            name: "Uptime and signed-in users",
            category: "Troubleshooting",
            description: "Shows how long the computer has been running and who is signed in.",
            os: vec![Macos, Linux],
            shell: Shell::Default,
            body: "uptime\necho\nwho",
            params: vec![],
        },
        ScriptTemplate {
            key: "any-reboot",
            name: "Restart the computer",
            category: "Power",
            description: "Restarts after a delay so signed-in users can save their work.",
            os: vec![Windows],
            shell: Shell::Powershell,
            body: "$m = [int]$env:OD_PARAM_MINUTES\nshutdown.exe /r /t ($m * 60) /c \"Your IT team is restarting this computer in $m minute(s). Please save your work.\"\n\"Restart scheduled in $m minute(s)\"",
            params: vec![param("MINUTES", "Delay (minutes)", "5", true)],
        },
        ScriptTemplate {
            key: "unix-reboot",
            name: "Restart the computer (macOS/Linux)",
            category: "Power",
            description: "Restarts after a delay so signed-in users can save their work.",
            os: vec![Macos, Linux],
            shell: Shell::Default,
            body: "shutdown -r \"+$OD_PARAM_MINUTES\" \"Your IT team is restarting this computer.\" && echo \"Restart scheduled in $OD_PARAM_MINUTES minute(s)\"",
            params: vec![param("MINUTES", "Delay (minutes)", "5", true)],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_params_are_marked_and_detected() {
        let t = templates().into_iter().find(|t| t.key == "win-domain-join").unwrap();
        let pw = t.params.iter().find(|p| p.name == "JOIN_PASSWORD").unwrap();
        assert!(pw.secret);
        assert!(is_secret_env("OD_PARAM_JOIN_PASSWORD"));
        assert!(is_secret_env("od_param_api_token"));
        assert!(!is_secret_env("OD_PARAM_DOMAIN"));
    }

    #[test]
    fn templates_are_well_formed() {
        let t = templates();
        let mut keys = std::collections::HashSet::new();
        for s in &t {
            assert!(keys.insert(s.key), "duplicate template key {}", s.key);
            assert!(!s.os.is_empty());
            for p in &s.params {
                assert!(valid_param_name(&p.name), "{}: bad param {}", s.key, p.name);
                assert!(s.body.contains(&format!("OD_PARAM_{}", p.name)), "{}: {} unused", s.key, p.name);
            }
        }
    }

    #[test]
    fn params_become_env_vars() {
        let ps = vec![param("NAME", "Name", "x", true), param("OPT", "Opt", "", false)];
        let mut v = BTreeMap::new();
        v.insert("NAME".to_string(), "bob; rm -rf /".to_string());
        let env = param_env(&ps, &v).unwrap();
        assert_eq!(env["OD_PARAM_NAME"], "bob; rm -rf /");
        assert_eq!(env["OD_PARAM_OPT"], "");
        assert!(param_env(&[param("R", "Req", "", true)], &BTreeMap::new()).is_err());
    }
}
