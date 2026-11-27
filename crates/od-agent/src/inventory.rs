//! Hardware and OS inventory.

use od_core::{Disk, Inventory, NetInterface};
use sysinfo::{Disks, Networks, System};

use crate::util;

pub async fn collect() -> Inventory {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let mut inv = Inventory {
        hostname: System::host_name().unwrap_or_default(),
        os_name: System::long_os_version().or_else(System::name).unwrap_or_default(),
        os_version: System::os_version().unwrap_or_default(),
        kernel_version: System::kernel_version().unwrap_or_default(),
        arch: System::cpu_arch(),
        cpu_model: sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
        cpu_cores: sys.cpus().len() as u32,
        memory_bytes: sys.total_memory(),
        uptime_secs: System::uptime(),
        ..Default::default()
    };

    let disks = Disks::new_with_refreshed_list();
    let mut seen = std::collections::HashSet::new();
    for d in disks.list() {
        let mount = d.mount_point().to_string_lossy().to_string();
        let fs = d.file_system().to_string_lossy().to_string();
        // Skip pseudo and duplicate filesystems (snaps, overlays, bind mounts).
        if d.total_space() == 0
            || ["squashfs", "overlay", "tmpfs", "devtmpfs"].contains(&fs.as_str())
            || mount.starts_with("/snap")
            || mount.starts_with("/System/Volumes/") && mount != "/System/Volumes/Data"
        {
            continue;
        }
        if !seen.insert((d.name().to_owned(), d.total_space())) {
            continue;
        }
        inv.disks.push(Disk { mount, total_bytes: d.total_space(), free_bytes: d.available_space(), file_system: fs });
    }

    let nets = Networks::new_with_refreshed_list();
    for (name, data) in nets.list() {
        let mac = data.mac_address().to_string();
        let ips: Vec<String> = data
            .ip_networks()
            .iter()
            .map(|n| n.addr)
            .filter(|a| !a.is_loopback() && !matches!(a, std::net::IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80))
            .map(|a| a.to_string())
            .collect();
        if ips.is_empty() || mac == "00:00:00:00:00:00" && name.starts_with("lo") {
            continue;
        }
        inv.interfaces.push(NetInterface { name: name.clone(), mac, ips });
    }
    inv.interfaces.sort_by(|a, b| a.name.cmp(&b.name));

    platform(&mut inv).await;
    inv
}

#[cfg(target_os = "linux")]
async fn platform(inv: &mut Inventory) {
    let dmi = |f: &str| std::fs::read_to_string(format!("/sys/class/dmi/id/{f}")).map(|s| s.trim().to_string()).unwrap_or_default();
    inv.manufacturer = dmi("sys_vendor");
    inv.model = dmi("product_name");
    inv.serial_number = dmi("product_serial");
    if let Ok(osr) = std::fs::read_to_string("/etc/os-release") {
        for line in osr.lines() {
            if let Some(v) = line.strip_prefix("PRETTY_NAME=") {
                inv.os_name = v.trim_matches('"').to_string();
            }
            if let Some(v) = line.strip_prefix("VARIANT=") {
                inv.os_edition = v.trim_matches('"').to_string();
            }
        }
    }
    inv.logged_in_users = who().await;
    inv.disk_encrypted = crate::policy::linux_root_encrypted().await;
    inv.firewall_enabled = crate::policy::linux_firewall_active().await;
}

#[cfg(target_os = "macos")]
async fn platform(inv: &mut Inventory) {
    inv.manufacturer = "Apple".into();
    if let Ok(o) = util::run("system_profiler", &["SPHardwareDataType"]).await {
        for line in o.stdout.lines() {
            let l = line.trim();
            if let Some(v) = l.strip_prefix("Serial Number (system):") {
                inv.serial_number = v.trim().into();
            } else if let Some(v) = l.strip_prefix("Model Name:") {
                inv.model = v.trim().into();
            } else if let Some(v) = l.strip_prefix("Model Identifier:") {
                if inv.model.is_empty() {
                    inv.model = v.trim().into();
                } else {
                    inv.model = format!("{} ({})", inv.model, v.trim());
                }
            }
        }
    }
    inv.logged_in_users = who().await;
    if let Ok(o) = util::run("fdesetup", &["status"]).await {
        inv.disk_encrypted = Some(o.stdout.contains("FileVault is On"));
    }
    if let Ok(o) = util::run("/usr/libexec/ApplicationFirewall/socketfilterfw", &["--getglobalstate"]).await {
        inv.firewall_enabled = Some(o.stdout.contains("enabled"));
    }
}

#[cfg(target_os = "windows")]
async fn platform(inv: &mut Inventory) {
    let script = r#"$ErrorActionPreference='SilentlyContinue'
$bios = Get-CimInstance Win32_BIOS; $cs = Get-CimInstance Win32_ComputerSystem; $os = Get-CimInstance Win32_OperatingSystem
$cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
$bl = $null; try { $v = Get-BitLockerVolume -MountPoint $env:SystemDrive; $bl = ($v.ProtectionStatus -eq 'On') } catch {}
$fw = $null; try { $fw = -not (Get-NetFirewallProfile | Where-Object { -not $_.Enabled }) } catch {}
$users = @(Get-CimInstance Win32_Process -Filter "Name='explorer.exe'" | ForEach-Object { (Invoke-CimMethod -InputObject $_ -MethodName GetOwner).User } | Sort-Object -Unique)
[pscustomobject]@{ serial=$bios.SerialNumber; manufacturer=$cs.Manufacturer; model=$cs.Model; caption=$os.Caption; edition=$cv.EditionID; display=$cv.DisplayVersion; build=$os.BuildNumber; bitlocker=$bl; firewall=$fw; users=$users } | ConvertTo-Json -Compress"#;
    if let Ok(o) = util::powershell(script, None).await {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(o.stdout.trim()) {
            let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
            inv.serial_number = s("serial");
            inv.manufacturer = s("manufacturer");
            inv.model = s("model");
            if !s("caption").is_empty() {
                inv.os_name = s("caption");
            }
            inv.os_edition = s("edition");
            if !s("display").is_empty() {
                inv.os_version = format!("{} (build {})", s("display"), s("build"));
            }
            inv.disk_encrypted = v.get("bitlocker").and_then(|x| x.as_bool());
            inv.firewall_enabled = v.get("firewall").and_then(|x| x.as_bool());
            inv.logged_in_users = v
                .get("users")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
        }
    }
}

/// Who is signed in, without collecting the rest of the inventory.
pub async fn logged_in_users() -> Vec<String> {
    #[cfg(unix)]
    {
        who().await
    }
    #[cfg(windows)]
    {
        let script = r#"@(Get-CimInstance Win32_Process -Filter "Name='explorer.exe'" | ForEach-Object { (Invoke-CimMethod -InputObject $_ -MethodName GetOwner).User } | Sort-Object -Unique) | ConvertTo-Json -Compress"#;
        let out = util::powershell(script, None).await.map(|o| o.stdout).unwrap_or_default();
        match serde_json::from_str::<serde_json::Value>(out.trim()) {
            Ok(serde_json::Value::Array(a)) => a.iter().filter_map(|u| u.as_str().map(str::to_string)).collect(),
            Ok(serde_json::Value::String(u)) => vec![u],
            _ => vec![],
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        vec![]
    }
}

#[cfg(unix)]
async fn who() -> Vec<String> {
    let mut users: Vec<String> = util::run("who", &[])
        .await
        .map(|o| o.stdout.lines().filter_map(|l| l.split_whitespace().next().map(str::to_string)).collect())
        .unwrap_or_default();
    users.sort();
    users.dedup();
    users
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
async fn platform(_inv: &mut Inventory) {}

#[allow(unused_imports)]
use util as _;
