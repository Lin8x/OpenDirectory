//! Installed software and package management: winget on Windows, Homebrew
//! on macOS, and apt, dnf, zypper, pacman, Flatpak and Snap on Linux. Also
//! the Software deployment GPO setting.

#[allow(unused_imports)]
use anyhow::{bail, Context, Result};
use od_core::manage::{PackageOp, PackageRule, SoftwareInfo, SoftwareReport};
use od_core::{ComplianceItem, ComplianceState};

#[allow(unused_imports)]
use super::must;
#[allow(unused_imports)]
use crate::util::which;

const INSTALL_TIMEOUT: u64 = 30 * 60;

/// Package managers on this endpoint, the default first.
pub async fn managers() -> Vec<String> {
    let mut v: Vec<&str> = vec![];
    #[cfg(target_os = "windows")]
    if winget().await.is_some() {
        v.push("winget");
    }
    #[cfg(target_os = "macos")]
    if brew().await.is_some() {
        v.extend(["brew", "brew_cask"]);
    }
    #[cfg(target_os = "linux")]
    {
        for (bin, m) in
            [("apt-get", "apt"), ("dnf", "dnf"), ("zypper", "zypper"), ("pacman", "pacman"), ("flatpak", "flatpak"), ("snap", "snap")]
        {
            if which(bin) {
                v.push(m);
            }
        }
    }
    v.into_iter().map(String::from).collect()
}

pub async fn list() -> Result<SoftwareReport> {
    let managers = managers().await;
    let mut notes = vec![];
    let mut items = vec![];
    collect(&managers, &mut items, &mut notes).await;
    items.sort_by_key(|a| a.name.to_lowercase());
    Ok(SoftwareReport { items, managers, notes })
}

fn sw(name: &str, version: &str, publisher: &str, source: &str, id: &str, manager: &str) -> SoftwareInfo {
    SoftwareInfo {
        name: name.trim().to_string(),
        version: version.trim().to_string(),
        publisher: publisher.trim().to_string(),
        source: source.into(),
        package_id: id.trim().to_string(),
        manager: manager.into(),
        update: String::new(),
    }
}

// ====================================================================== Linux
#[cfg(target_os = "linux")]
async fn collect(managers: &[String], items: &mut Vec<SoftwareInfo>, notes: &mut Vec<String>) {
    use std::collections::HashMap;
    let has = |m: &str| managers.iter().any(|x| x == m);
    let native = ["apt", "dnf", "zypper", "pacman"].into_iter().find(|m| has(m)).unwrap_or("");
    let mut updates: HashMap<String, String> = HashMap::new();
    if which("dpkg-query") {
        match must("dpkg-query", &["-W", "-f=${db:Status-Abbrev}\t${Package}\t${Version}\t${Maintainer}\n"], 120).await {
            Ok(out) => {
                for l in out.lines() {
                    let f: Vec<&str> = l.split('\t').collect();
                    if f.len() >= 4 && f[0].starts_with("ii") {
                        items.push(sw(f[1], f[2], f[3], "apt", f[1], native));
                    }
                }
            }
            Err(e) => notes.push(format!("dpkg: {e:#}")),
        }
        if has("apt") {
            if let Ok(out) = must("apt", &["list", "--upgradable"], 120).await {
                updates.extend(parse_apt_upgradable(&out));
            }
        }
    } else if which("rpm") {
        match must("rpm", &["-qa", "--qf", "%{NAME}\t%{VERSION}-%{RELEASE}\t%{VENDOR}\n"], 120).await {
            Ok(out) => {
                for l in out.lines() {
                    let f: Vec<&str> = l.split('\t').collect();
                    if f.len() >= 3 && !f[0].starts_with("gpg-pubkey") {
                        items.push(sw(f[0], f[1], f[2].trim_start_matches("(none)"), "rpm", f[0], native));
                    }
                }
            }
            Err(e) => notes.push(format!("rpm: {e:#}")),
        }
        if has("dnf") {
            // Exit code 100 means updates are available.
            if let Ok(o) = crate::util::run_with("dnf", &["check-update", "-q"], None, std::time::Duration::from_secs(300)).await {
                for l in o.stdout.lines() {
                    let f: Vec<&str> = l.split_whitespace().collect();
                    if f.len() >= 3 {
                        let name = f[0].rsplit_once('.').map(|x| x.0).unwrap_or(f[0]);
                        updates.insert(name.to_string(), f[1].to_string());
                    }
                }
            }
        }
    } else if has("pacman") {
        if let Ok(out) = must("pacman", &["-Q"], 120).await {
            for l in out.lines() {
                if let Some((n, v)) = l.split_once(' ') {
                    items.push(sw(n, v, "", "pacman", n, "pacman"));
                }
            }
        }
        if let Ok(o) = crate::util::run("pacman", &["-Qu"]).await {
            for l in o.stdout.lines() {
                let f: Vec<&str> = l.split_whitespace().collect();
                if f.len() >= 4 {
                    updates.insert(f[0].into(), f[3].into());
                }
            }
        }
    }
    for i in items.iter_mut() {
        if let Some(v) = updates.get(&i.package_id) {
            i.update = v.clone();
        }
    }
    if has("flatpak") {
        if let Ok(out) = must("flatpak", &["list", "--app", "--columns=name,application,version,origin"], 120).await {
            let ups: HashMap<String, String> = must("flatpak", &["remote-ls", "--updates", "--columns=application,version"], 120)
                .await
                .unwrap_or_default()
                .lines()
                .filter_map(|l| l.split_once('\t').map(|(a, v)| (a.to_string(), v.to_string())))
                .collect();
            for l in out.lines() {
                let f: Vec<&str> = l.split('\t').collect();
                if f.len() >= 4 {
                    let mut s = sw(f[0], f[2], f[3], "flatpak", f[1], "flatpak");
                    s.update = ups.get(f[1]).cloned().unwrap_or_default();
                    items.push(s);
                }
            }
        }
    }
    if has("snap") {
        if let Ok(out) = must("snap", &["list"], 120).await {
            let ups: HashMap<String, String> = crate::util::run("snap", &["refresh", "--list"])
                .await
                .map(|o| o.stdout)
                .unwrap_or_default()
                .lines()
                .skip(1)
                .filter_map(|l| {
                    let f: Vec<&str> = l.split_whitespace().collect();
                    (f.len() >= 2).then(|| (f[0].to_string(), f[1].to_string()))
                })
                .collect();
            for l in out.lines().skip(1) {
                let f: Vec<&str> = l.split_whitespace().collect();
                if f.len() >= 5 {
                    let mut s = sw(f[0], f[1], f[4].trim_end_matches('✓').trim_end_matches('*'), "snap", f[0], "snap");
                    s.update = ups.get(f[0]).cloned().unwrap_or_default();
                    items.push(s);
                }
            }
        }
    }
}

/// `apt list --upgradable`: "name/suite version arch [upgradable from: old]".
#[cfg(any(target_os = "linux", test))]
fn parse_apt_upgradable(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter(|l| l.contains("[upgradable from"))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?.split('/').next()?.to_string();
            let ver = it.next()?.to_string();
            Some((name, ver))
        })
        .collect()
}

#[cfg(target_os = "linux")]
async fn run_manager(op: PackageOp, name: &str, manager: &str) -> Result<String> {
    let env: std::collections::BTreeMap<String, String> =
        [("DEBIAN_FRONTEND".to_string(), "noninteractive".to_string())].into_iter().collect();
    let (prog, args): (&str, Vec<&str>) = match (manager, op) {
        ("apt", PackageOp::Install) => ("apt-get", vec!["install", "-y", "--", name]),
        ("apt", PackageOp::Update) => ("apt-get", vec!["install", "--only-upgrade", "-y", "--", name]),
        ("apt", PackageOp::Remove) => ("apt-get", vec!["remove", "-y", "--", name]),
        ("dnf", PackageOp::Install) => ("dnf", vec!["install", "-y", name]),
        ("dnf", PackageOp::Update) => ("dnf", vec!["upgrade", "-y", name]),
        ("dnf", PackageOp::Remove) => ("dnf", vec!["remove", "-y", name]),
        ("zypper", PackageOp::Install) => ("zypper", vec!["--non-interactive", "install", name]),
        ("zypper", PackageOp::Update) => ("zypper", vec!["--non-interactive", "update", name]),
        ("zypper", PackageOp::Remove) => ("zypper", vec!["--non-interactive", "remove", name]),
        ("pacman", PackageOp::Install | PackageOp::Update) => ("pacman", vec!["-S", "--noconfirm", "--needed", name]),
        ("pacman", PackageOp::Remove) => ("pacman", vec!["-R", "--noconfirm", name]),
        ("flatpak", PackageOp::Install) => ("flatpak", vec!["install", "-y", "--noninteractive", "flathub", name]),
        ("flatpak", PackageOp::Update) => ("flatpak", vec!["update", "-y", "--noninteractive", name]),
        ("flatpak", PackageOp::Remove) => ("flatpak", vec!["uninstall", "-y", "--noninteractive", name]),
        ("snap", PackageOp::Install) => ("snap", vec!["install", name]),
        ("snap", PackageOp::Update) => ("snap", vec!["refresh", name]),
        ("snap", PackageOp::Remove) => ("snap", vec!["remove", name]),
        (m, _) => bail!("{m} isn't available on Linux"),
    };
    let o = crate::util::run_env(prog, &args, &env, std::time::Duration::from_secs(INSTALL_TIMEOUT)).await?;
    let out = super::combined(&o);
    if !o.ok() {
        bail!("{prog} failed:\n{}", tail(&out));
    }
    Ok(tail(&out))
}

// ==================================================================== Windows
#[cfg(target_os = "windows")]
pub async fn winget() -> Option<String> {
    // The SYSTEM account has no winget alias; find the App Installer binary directly.
    let find = r#"$w = Get-ChildItem "$env:ProgramFiles\WindowsApps\Microsoft.DesktopAppInstaller_*_x64__8wekyb3d8bbwe\winget.exe" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1; if ($w) { $w.FullName } elseif (Get-Command winget -ErrorAction SilentlyContinue) { 'winget' }"#;
    let w = crate::util::powershell(find, None).await.ok()?.stdout.trim().to_string();
    (!w.is_empty()).then_some(w)
}

#[cfg(target_os = "windows")]
async fn collect(managers: &[String], items: &mut Vec<SoftwareInfo>, notes: &mut Vec<String>) {
    // Programs and Features (the Uninstall registry keys) is the complete list.
    let ps = r#"$k='HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*','HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'; @(Get-ItemProperty $k -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -and -not $_.SystemComponent -and -not $_.ParentKeyName } | ForEach-Object { [pscustomobject]@{ name=[string]$_.DisplayName; version=[string]$_.DisplayVersion; publisher=[string]$_.Publisher } }) | ConvertTo-Json -Compress"#;
    #[derive(serde::Deserialize)]
    struct Row {
        name: String,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        publisher: Option<String>,
    }
    match crate::util::powershell(ps, None).await {
        Ok(o) if o.ok() => {
            let rows: Vec<Row> = serde_json::from_str(o.stdout.trim()).unwrap_or_default();
            for r in rows {
                items.push(sw(&r.name, r.version.as_deref().unwrap_or(""), r.publisher.as_deref().unwrap_or(""), "programs", "", ""));
            }
        }
        Ok(o) => notes.push(format!("Programs and Features: {}", o.message())),
        Err(e) => notes.push(format!("Programs and Features: {e:#}")),
    }
    if !managers.iter().any(|m| m == "winget") {
        notes.push("winget (App Installer) isn't installed, so apps can't be installed or updated here".into());
        return;
    }
    let Some(w) = winget().await else { return };
    match crate::util::run_with(
        &w,
        &["list", "--accept-source-agreements", "--disable-interactivity"],
        None,
        std::time::Duration::from_secs(300),
    )
    .await
    {
        Ok(o) => {
            for row in parse_winget_table(&o.stdout) {
                // Link the winget id to the Programs and Features entry.
                let found = items.iter().position(|i| i.package_id.is_empty() && (i.name == row.name || row.name.starts_with(&i.name)));
                let idx = match found {
                    Some(i) => i,
                    None => {
                        items.push(sw(&row.name, &row.version, "", "winget", "", ""));
                        items.len() - 1
                    }
                };
                let target = &mut items[idx];
                target.package_id = row.id;
                target.manager = "winget".into();
                target.update = row.available;
                if !row.source.is_empty() {
                    target.source = "winget".into();
                }
            }
        }
        Err(e) => notes.push(format!("winget list: {e:#}")),
    }
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, PartialEq)]
struct WingetRow {
    name: String,
    id: String,
    version: String,
    available: String,
    source: String,
}

/// winget prints a fixed-width table: find the columns from the header.
#[cfg(any(target_os = "windows", test))]
fn parse_winget_table(out: &str) -> Vec<WingetRow> {
    // Drop the progress spinner (it redraws with carriage returns).
    let lines: Vec<String> = out.lines().map(|l| l.rsplit('\r').next().unwrap_or("").to_string()).collect();
    let Some(h) = lines.iter().position(|l| l.trim_start().starts_with("Name") && l.contains(" Id ") && l.contains("Version")) else {
        return vec![];
    };
    let header: Vec<char> = lines[h].chars().collect();
    let col = |name: &str| -> Option<usize> {
        let s: String = header.iter().collect();
        s.find(&format!(" {name}")).map(|b| s[..b + 1].chars().count())
    };
    let (Some(id), Some(ver)) = (col("Id"), col("Version")) else { return vec![] };
    let avail = col("Available");
    let src = col("Source");
    let cut = |chars: &[char], a: usize, b: Option<usize>| -> String {
        let b = b.unwrap_or(chars.len()).min(chars.len());
        if a >= b {
            return String::new();
        }
        chars[a..b].iter().collect::<String>().trim().to_string()
    };
    lines[h + 1..]
        .iter()
        .filter(|l| !l.trim().is_empty() && !l.starts_with("---") && !l.contains("upgrades available"))
        .filter_map(|l| {
            let c: Vec<char> = l.chars().collect();
            let version_end = avail.or(src);
            let r = WingetRow {
                name: cut(&c, 0, Some(id)),
                id: cut(&c, id, Some(ver)),
                version: cut(&c, ver, version_end),
                available: avail.map(|a| cut(&c, a, src)).unwrap_or_default(),
                source: src.map(|s| cut(&c, s, None)).unwrap_or_default(),
            };
            (!r.id.is_empty()).then_some(r)
        })
        .collect()
}

#[cfg(target_os = "windows")]
async fn run_manager(op: PackageOp, name: &str, manager: &str) -> Result<String> {
    if manager != "winget" {
        bail!("{manager} isn't available on Windows");
    }
    let Some(w) = winget().await else { bail!("winget (App Installer) isn't installed on this endpoint") };
    let verb = match op {
        PackageOp::Install => "install",
        PackageOp::Update => "upgrade",
        PackageOp::Remove => "uninstall",
    };
    let mut args = vec![verb, "--id", name, "--exact", "--silent", "--accept-source-agreements", "--disable-interactivity"];
    if op != PackageOp::Remove {
        args.push("--accept-package-agreements");
    }
    let timeout = std::time::Duration::from_secs(INSTALL_TIMEOUT);
    let mut machine = args.clone();
    if op == PackageOp::Install {
        machine.extend(["--scope", "machine"]);
    }
    let mut o = crate::util::run_with(&w, &machine, None, timeout).await?;
    if !o.ok() && op == PackageOp::Install {
        // Some packages only have a per-user installer.
        o = crate::util::run_with(&w, &args, None, timeout).await?;
    }
    let out = super::combined(&o);
    let out: String =
        out.lines().map(|l| l.rsplit('\r').next().unwrap_or("")).filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n");
    if !o.ok() {
        bail!("winget {verb} failed (exit {}):\n{}", o.code.unwrap_or(-1), tail(&out));
    }
    Ok(tail(&out))
}

// ====================================================================== macOS
#[cfg(target_os = "macos")]
pub async fn brew() -> Option<(String, String)> {
    use std::os::unix::fs::MetadataExt;
    let path = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].into_iter().find(|p| std::path::Path::new(p).exists())?;
    let uid = std::fs::metadata(path).ok()?.uid();
    let owner = crate::util::run("id", &["-nu", &uid.to_string()]).await.ok()?.stdout.trim().to_string();
    (!owner.is_empty()).then(|| (path.to_string(), owner))
}

#[cfg(target_os = "macos")]
async fn brew_run(args: &[&str], timeout: u64) -> Result<crate::util::Output> {
    let Some((b, owner)) = brew().await else { bail!("Homebrew isn't installed on this endpoint") };
    // Homebrew refuses to run as root; run it as the user who owns it.
    let mut a = vec!["-H", "-u", owner.as_str(), b.as_str()];
    a.extend_from_slice(args);
    crate::util::run_with("sudo", &a, None, std::time::Duration::from_secs(timeout)).await
}

#[cfg(target_os = "macos")]
async fn collect(managers: &[String], items: &mut Vec<SoftwareInfo>, notes: &mut Vec<String>) {
    use std::collections::HashMap;
    // App bundles in /Applications.
    if let Ok(rd) = std::fs::read_dir("/Applications") {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("app") {
                continue;
            }
            let plist = p.join("Contents/Info.plist");
            let v = crate::util::run("plutil", &["-extract", "CFBundleShortVersionString", "raw", "-o", "-", &plist.to_string_lossy()])
                .await
                .map(|o| o.stdout.trim().to_string())
                .unwrap_or_default();
            let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            items.push(sw(&name, &v, "", "app", "", ""));
        }
    }
    if !managers.iter().any(|m| m == "brew") {
        notes.push("Homebrew isn't installed, so apps can't be installed or updated here".into());
        return;
    }
    let mut outdated: HashMap<String, String> = HashMap::new();
    if let Ok(o) = brew_run(&["outdated", "--json=v2"], 300).await {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&o.stdout) {
            for k in ["formulae", "casks"] {
                for x in v[k].as_array().into_iter().flatten() {
                    if let (Some(n), Some(c)) = (x["name"].as_str(), x["current_version"].as_str()) {
                        outdated.insert(n.into(), c.into());
                    }
                }
            }
        }
    }
    for (flag, mgr) in [("--formula", "brew"), ("--cask", "brew_cask")] {
        match brew_run(&["list", flag, "--versions"], 120).await {
            Ok(o) if o.ok() => {
                for l in o.stdout.lines() {
                    let mut it = l.split_whitespace();
                    let Some(n) = it.next() else { continue };
                    let v = it.last().unwrap_or("");
                    let mut s = sw(n, v, "", mgr, n, mgr);
                    s.update = outdated.get(n).cloned().unwrap_or_default();
                    items.push(s);
                }
            }
            Ok(o) => notes.push(format!("brew list {flag}: {}", o.message())),
            Err(e) => notes.push(format!("brew list {flag}: {e:#}")),
        }
    }
}

#[cfg(target_os = "macos")]
async fn run_manager(op: PackageOp, name: &str, manager: &str) -> Result<String> {
    let verb = match op {
        PackageOp::Install => "install",
        PackageOp::Update => "upgrade",
        PackageOp::Remove => "uninstall",
    };
    let args: Vec<&str> = match manager {
        "brew" => vec![verb, "--formula", name],
        "brew_cask" => vec![verb, "--cask", name],
        m => bail!("{m} isn't available on macOS"),
    };
    let o = brew_run(&args, INSTALL_TIMEOUT).await?;
    let out = super::combined(&o);
    if !o.ok() {
        bail!("brew {verb} failed:\n{}", tail(&out));
    }
    Ok(tail(&out))
}

// =================================================================== shared

/// The last 60 lines, which is where package managers put the outcome.
fn tail(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(60)..].join("\n")
}

async fn resolve(manager: &str) -> Result<String> {
    let have = managers().await;
    if manager.is_empty() {
        return have.into_iter().next().context("No supported package manager is installed on this endpoint");
    }
    if !have.iter().any(|m| m == manager) {
        bail!("{manager} isn't installed on this endpoint (available: {})", if have.is_empty() { "none".into() } else { have.join(", ") });
    }
    Ok(manager.to_string())
}

pub async fn package(op: PackageOp, name: &str, manager: &str) -> Result<String> {
    let m = resolve(manager).await?;
    run_manager(op, name, &m).await
}

fn rule_applies(r: &PackageRule) -> bool {
    r.os.map_or(true, |os| os == od_core::OsFamily::current())
}

/// The Software deployment GPO setting: install, keep updated or uninstall.
pub async fn deploy(rules: &[PackageRule]) -> ComplianceItem {
    let kind = "software_deployment".to_string();
    let item = |state, message: String| ComplianceItem { setting: kind.clone(), state, message };
    let rules: Vec<&PackageRule> = rules.iter().filter(|r| rule_applies(r)).collect();
    if rules.is_empty() {
        return item(ComplianceState::Compliant, "No apps are deployed to this platform".into());
    }
    let report = match list().await {
        Ok(r) => r,
        Err(e) => return item(ComplianceState::Failed, format!("Couldn't read installed software: {e:#}")),
    };
    let mut changed = vec![];
    let mut failed = vec![];
    for r in rules {
        let m = match resolve(&r.manager).await {
            Ok(m) => m,
            Err(e) => {
                failed.push(format!("{}: {e:#}", r.name));
                continue;
            }
        };
        let found = report.items.iter().find(|i| {
            i.package_id.eq_ignore_ascii_case(&r.name)
                && (i.manager == m || i.manager.is_empty() || m.starts_with("brew") && i.manager.starts_with("brew"))
        });
        let op = match (r.ensure.as_str(), found) {
            ("absent", Some(_)) => Some(PackageOp::Remove),
            ("absent", None) => None,
            (_, None) => Some(PackageOp::Install),
            ("latest", Some(i)) if !i.update.is_empty() => Some(PackageOp::Update),
            _ => None,
        };
        let Some(op) = op else { continue };
        match run_manager(op, &r.name, &m).await {
            Ok(_) => changed.push(format!(
                "{} {}",
                match op {
                    PackageOp::Install => "installed",
                    PackageOp::Update => "updated",
                    PackageOp::Remove => "uninstalled",
                },
                r.name
            )),
            Err(e) => failed.push(format!("{}: {}", r.name, e.to_string().lines().last().unwrap_or(""))),
        }
    }
    if !failed.is_empty() {
        let mut msg = failed.join("; ");
        if !changed.is_empty() {
            msg = format!("{msg} (also {})", changed.join(", "));
        }
        item(ComplianceState::Failed, msg)
    } else if !changed.is_empty() {
        item(ComplianceState::Applied, changed.join(", "))
    } else {
        item(ComplianceState::Compliant, "Every deployed app is in the required state".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_winget_list() {
        let out = "\r-\r\\\rName                      Id                     Version      Available   Source\n\
                   ---------------------------------------------------------------------------------\n\
                   Mozilla Firefox (x64 en)  Mozilla.Firefox        120.0        121.0       winget\n\
                   Contoso Tool              ARP\\Machine\\X64\\Tool   1.2                       \n\
                   2 upgrades available.\n";
        let rows = parse_winget_table(out);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].name, "Mozilla Firefox (x64 en)");
        assert_eq!(rows[0].id, "Mozilla.Firefox");
        assert_eq!(rows[0].version, "120.0");
        assert_eq!(rows[0].available, "121.0");
        assert_eq!(rows[0].source, "winget");
        assert_eq!(rows[1].id, "ARP\\Machine\\X64\\Tool");
        assert_eq!(rows[1].available, "");
    }

    #[test]
    fn parses_apt_upgradable() {
        let out = "Listing...\nopenssl/noble-updates 3.0.13-0ubuntu3.4 amd64 [upgradable from: 3.0.13-0ubuntu3.1]\n";
        assert_eq!(parse_apt_upgradable(out), vec![("openssl".to_string(), "3.0.13-0ubuntu3.4".to_string())]);
    }
}
