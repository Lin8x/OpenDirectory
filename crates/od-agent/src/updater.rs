//! Self-update. When the Management Server offers a newer agent, download it
//! over the pinned connection, check its SHA-256, swap the binary (keeping
//! the old one), and restart through the service manager. If the new
//! version fails to check in after a few starts, put the old one back.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use od_core::protocol::AgentUpdate;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agent::VERSION;
use crate::state::data_dir;

/// This build's platform name, matching the release asset names.
pub const PLATFORM: &str = if cfg!(all(windows, target_arch = "x86_64")) {
    "windows-x64"
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    "macos-arm64"
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    "macos-x64"
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    "linux-x64"
} else {
    ""
};

/// Starts allowed without a successful check-in before rolling back.
const MAX_STARTS: u32 = 3;

#[derive(Serialize, Deserialize, Default)]
struct Pending {
    from: String,
    to: String,
    started_at: Option<DateTime<Utc>>,
    starts: u32,
}

#[derive(Serialize, Deserialize, Default)]
struct Failed {
    version: String,
    at: Option<DateTime<Utc>>,
}

fn pending_path() -> PathBuf {
    data_dir().join("update-pending.json")
}
fn failed_path() -> PathBuf {
    data_dir().join("update-failed.json")
}
fn old_path(exe: &Path) -> PathBuf {
    let mut p = exe.as_os_str().to_owned();
    p.push(".old");
    PathBuf::from(p)
}
fn read<T: for<'de> Deserialize<'de> + Default>(p: &Path) -> Option<T> {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// At startup: count starts of a freshly installed version and roll back if
/// it keeps failing. Returns true if the agent should exit to restart.
pub fn on_start() -> bool {
    let Some(mut p) = read::<Pending>(&pending_path()) else { return false };
    if p.to != VERSION {
        // We're the old binary again (rolled back or replaced by hand).
        let _ = std::fs::remove_file(pending_path());
        return false;
    }
    p.starts += 1;
    if p.starts > MAX_STARTS {
        tracing::error!("Endpoint Agent {VERSION} didn't check in after {MAX_STARTS} starts; restoring {}", p.from);
        let ok = std::env::current_exe().map(|exe| rollback(&exe)).unwrap_or(false);
        let _ =
            std::fs::write(failed_path(), serde_json::to_vec(&Failed { version: p.to.clone(), at: Some(Utc::now()) }).unwrap_or_default());
        let _ = std::fs::remove_file(pending_path());
        return ok;
    }
    let _ = std::fs::write(pending_path(), serde_json::to_vec(&p).unwrap_or_default());
    false
}

/// After a successful check-in: the update worked, so drop the backup.
pub fn confirm() {
    if let Some(p) = read::<Pending>(&pending_path()) {
        if p.to == VERSION {
            tracing::info!("Updated the Endpoint Agent from {} to {VERSION}", p.from);
            let _ = std::fs::remove_file(pending_path());
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::fs::remove_file(old_path(&exe));
            }
        }
    }
}

fn rollback(exe: &Path) -> bool {
    let old = old_path(exe);
    if !old.exists() {
        return false;
    }
    if cfg!(windows) {
        let mut bad = exe.as_os_str().to_owned();
        bad.push(".failed");
        let _ = std::fs::remove_file(PathBuf::from(&bad));
        if std::fs::rename(exe, PathBuf::from(&bad)).is_err() {
            return false;
        }
    }
    std::fs::rename(&old, exe).is_ok()
}

/// Was this version tried recently and rolled back?
fn recently_failed(version: &str) -> bool {
    read::<Failed>(&failed_path())
        .map(|f| f.version == version && f.at.map(|t| Utc::now() - t < chrono::Duration::hours(24)).unwrap_or(false))
        .unwrap_or(false)
}

/// Download, verify and install an offered update. Returns Ok(true) when the
/// agent should exit so the service manager starts the new version.
pub async fn apply(update: &AgentUpdate, client: &od_client::Client) -> Result<bool> {
    if PLATFORM.is_empty() || update.version == VERSION || recently_failed(&update.version) {
        return Ok(false);
    }
    if !od_core::policy::version_at_least(&update.version, VERSION) {
        return Ok(false);
    }
    tracing::info!("Downloading Endpoint Agent {} ({} bytes)", update.version, update.size);
    let bytes = client.get_bytes(&format!("/agent/update/{PLATFORM}")).await.context("download")?;
    if bytes.len() as u64 != update.size {
        bail!("The download is {} bytes; expected {}", bytes.len(), update.size);
    }
    let sha = hex::encode(Sha256::digest(&bytes));
    if !sha.eq_ignore_ascii_case(&update.sha256) {
        bail!("The download's SHA-256 doesn't match; not installing it");
    }
    let exe = std::env::current_exe().context("find the agent binary")?;
    install(&exe, &bytes)?;
    let p = Pending { from: VERSION.into(), to: update.version.clone(), started_at: Some(Utc::now()), starts: 0 };
    std::fs::write(pending_path(), serde_json::to_vec(&p)?)?;
    tracing::info!("Installed Endpoint Agent {}; restarting", update.version);
    Ok(true)
}

fn install(exe: &Path, bytes: &[u8]) -> Result<()> {
    let mut new = exe.as_os_str().to_owned();
    new.push(".new");
    let new = PathBuf::from(new);
    std::fs::write(&new, bytes).with_context(|| format!("write {}", new.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))?;
    }
    let old = old_path(exe);
    let _ = std::fs::remove_file(&old);
    if cfg!(windows) {
        // A running .exe can be renamed but not overwritten.
        std::fs::rename(exe, &old).context("move the running agent aside")?;
    } else {
        std::fs::copy(exe, &old).context("keep a copy of the current agent")?;
    }
    if let Err(e) = std::fs::rename(&new, exe) {
        if cfg!(windows) {
            let _ = std::fs::rename(&old, exe);
        }
        return Err(e).context("install the new agent");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_swaps_and_rollback_restores() {
        let dir = std::env::temp_dir().join(format!("od-upd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("opendirectory-agent");
        std::fs::write(&exe, b"old build").unwrap();
        install(&exe, b"new build").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new build");
        assert_eq!(std::fs::read(old_path(&exe)).unwrap(), b"old build");
        assert!(rollback(&exe));
        assert_eq!(std::fs::read(&exe).unwrap(), b"old build");
        let _ = std::fs::remove_dir_all(dir);
    }
}
