//! One-file configuration backups (`.odbackup`) and restore.
//!
//! A backup holds a consistent copy of the directory database plus the keys
//! and certificate a restored server needs (`master.key` can decrypt every
//! user's sealed password, so backups are encrypted by default).
//!
//! File layout:
//! ```text
//! ODBACKUP1\n
//! {json header}\n
//! <payload: gzip(JSON bundle), or XChaCha20-Poly1305(gzip(...)) when encrypted>
//! ```
//! The key for an encrypted backup is derived from the passphrase with Argon2id.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::db::Db;

const MAGIC: &[u8] = b"ODBACKUP1\n";
/// Files in the data directory that make up a server, besides the database.
const FILES: &[&str] = &["master.key", "tls-cert.pem", "tls-key.pem"];
const DB_FILE: &str = "directory.db";
pub const RESTORE_DIR: &str = "restore-pending";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub format: u32,
    pub created_at: DateTime<Utc>,
    pub server_version: String,
    pub organization: Option<String>,
    pub domain: Option<String>,
    pub encrypted: bool,
    /// Argon2id parameters and salt (base64), when encrypted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kdf: Option<Kdf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kdf {
    pub algorithm: String,
    pub salt: String,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

#[derive(Serialize, Deserialize)]
struct Bundle {
    /// File name -> base64 contents.
    files: BTreeMap<String, String>,
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn derive_key(passphrase: &str, kdf: &Kdf) -> Result<[u8; 32]> {
    let salt = b64().decode(&kdf.salt)?;
    let params = argon2::Params::new(kdf.memory_kib, kdf.iterations, kdf.parallelism, Some(32)).map_err(|e| anyhow!("{e}"))?;
    let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = [0u8; 32];
    a.hash_password_into(passphrase.as_bytes(), &salt, &mut key).map_err(|e| anyhow!("derive key: {e}"))?;
    Ok(key)
}

/// File name for a new backup, e.g. `opendirectory-corp.contoso.com-20261125-1830.odbackup`.
pub fn file_name(db: &Db) -> String {
    let domain = db.meta_get("domain").ok().flatten().unwrap_or_else(|| "server".into());
    let safe: String = domain.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '-' }).collect();
    format!("opendirectory-{safe}-{}.odbackup", Utc::now().format("%Y%m%d-%H%M%S"))
}

/// Make a backup of the running server.
pub fn create(db: &Db, data_dir: &Path, passphrase: Option<&str>) -> Result<Vec<u8>> {
    if let Some(p) = passphrase {
        if p.chars().count() < 12 {
            bail!("Use a backup passphrase of at least 12 characters");
        }
    }
    let mut files = BTreeMap::new();
    files.insert(DB_FILE.to_string(), b64().encode(db.snapshot()?));
    for f in FILES {
        if let Ok(bytes) = std::fs::read(data_dir.join(f)) {
            files.insert(f.to_string(), b64().encode(bytes));
        }
    }
    let json = serde_json::to_vec(&Bundle { files })?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&json)?;
    let plain = gz.finish()?;

    let mut header = Header {
        format: 1,
        created_at: Utc::now(),
        server_version: crate::VERSION.into(),
        organization: db.meta_get("organization")?,
        domain: db.meta_get("domain")?,
        encrypted: passphrase.is_some(),
        kdf: None,
        nonce: None,
    };
    let payload = match passphrase {
        None => plain,
        Some(p) => {
            let mut salt = [0u8; 16];
            let mut nonce = [0u8; 24];
            rand::thread_rng().fill_bytes(&mut salt);
            rand::thread_rng().fill_bytes(&mut nonce);
            let kdf = Kdf { algorithm: "argon2id".into(), salt: b64().encode(salt), memory_kib: 65536, iterations: 3, parallelism: 1 };
            let key = derive_key(p, &kdf)?;
            let cipher = XChaCha20Poly1305::new_from_slice(&key).map_err(|e| anyhow!("{e}"))?;
            // The header is authenticated too, so it can't be altered.
            header.kdf = Some(kdf);
            header.nonce = Some(b64().encode(nonce));
            let aad = serde_json::to_vec(&header)?;
            cipher
                .encrypt(XNonce::from_slice(&nonce), chacha20poly1305::aead::Payload { msg: &plain, aad: &aad })
                .map_err(|e| anyhow!("encrypt: {e}"))?
        }
    };
    let mut out = MAGIC.to_vec();
    out.extend(serde_json::to_vec(&header)?);
    out.push(b'\n');
    out.extend(payload);
    Ok(out)
}

/// Read just the header (to show what a file is before asking for a passphrase).
pub fn read_header(bytes: &[u8]) -> Result<Header> {
    Ok(split(bytes)?.0)
}

fn split(bytes: &[u8]) -> Result<(Header, &[u8], &[u8])> {
    let rest = bytes.strip_prefix(MAGIC).ok_or_else(|| anyhow!("This isn't an OpenDirectory backup file"))?;
    let nl = rest.iter().position(|b| *b == b'\n').ok_or_else(|| anyhow!("The backup file is damaged"))?;
    let raw = &rest[..nl];
    let header: Header = serde_json::from_slice(raw).context("The backup header is damaged")?;
    if header.format != 1 {
        bail!("This backup was made by a newer version of OpenDirectory (format {})", header.format);
    }
    Ok((header, raw, &rest[nl + 1..]))
}

/// Decrypt and unpack a backup into its files.
pub fn open(bytes: &[u8], passphrase: Option<&str>) -> Result<(Header, BTreeMap<String, Vec<u8>>)> {
    let (header, _raw, payload) = split(bytes)?;
    let plain = if header.encrypted {
        let p = passphrase.filter(|p| !p.is_empty()).ok_or_else(|| anyhow!("This backup is encrypted; enter its passphrase"))?;
        let kdf = header.kdf.as_ref().ok_or_else(|| anyhow!("The backup header is damaged"))?;
        let nonce = b64().decode(header.nonce.as_deref().unwrap_or(""))?;
        if nonce.len() != 24 {
            bail!("The backup header is damaged");
        }
        let key = derive_key(p, kdf)?;
        let cipher = XChaCha20Poly1305::new_from_slice(&key).map_err(|e| anyhow!("{e}"))?;
        let aad = serde_json::to_vec(&header)?;
        cipher
            .decrypt(XNonce::from_slice(&nonce), chacha20poly1305::aead::Payload { msg: payload, aad: &aad })
            .map_err(|_| anyhow!("Wrong passphrase, or the file was changed"))?
    } else {
        payload.to_vec()
    };
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(&plain[..]).read_to_end(&mut json).context("The backup contents are damaged")?;
    let bundle: Bundle = serde_json::from_slice(&json).context("The backup contents are damaged")?;
    let mut files = BTreeMap::new();
    for (k, v) in bundle.files {
        if k != DB_FILE && !FILES.contains(&k.as_str()) {
            continue;
        }
        files.insert(k, b64().decode(v)?);
    }
    if !files.contains_key(DB_FILE) {
        bail!("The backup has no directory database");
    }
    Ok((header, files))
}

#[derive(Debug, Clone, Serialize)]
pub struct Verified {
    pub header: Header,
    pub users: i64,
    pub groups: i64,
    pub endpoints: i64,
    pub policies: i64,
    pub size_bytes: usize,
}

/// Decrypt a backup and check its database opens and passes SQLite's integrity check.
pub fn verify(bytes: &[u8], passphrase: Option<&str>) -> Result<Verified> {
    let (header, files) = open(bytes, passphrase)?;
    let dir = std::env::temp_dir().join(format!("od-verify-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    let res = (|| -> Result<Verified> {
        let p = dir.join(DB_FILE);
        std::fs::write(&p, &files[DB_FILE])?;
        let c = rusqlite::Connection::open(&p)?;
        let ok: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if ok != "ok" {
            bail!("The database in this backup is damaged: {ok}");
        }
        let n = |t: &str| c.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get::<_, i64>(0)).unwrap_or(0);
        Ok(Verified {
            header: header.clone(),
            users: n("users"),
            groups: n("groups"),
            endpoints: n("endpoints"),
            policies: n("policies"),
            size_bytes: bytes.len(),
        })
    })();
    let _ = std::fs::remove_dir_all(&dir);
    res
}

/// Stage a restore. It's applied the next time the Management Server starts,
/// so the running database is never swapped underneath open connections.
pub fn stage_restore(data_dir: &Path, bytes: &[u8], passphrase: Option<&str>) -> Result<Verified> {
    let v = verify(bytes, passphrase)?;
    let (_, files) = open(bytes, passphrase)?;
    let dir = data_dir.join(RESTORE_DIR);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    for (name, data) in files {
        crate::crypto::write_private(&dir.join(name), &data)?;
    }
    Ok(v)
}

/// Apply a staged restore, keeping the replaced files in `pre-restore-<time>/`.
/// Called at startup before the database is opened.
pub fn apply_pending(data_dir: &Path) -> Result<Option<PathBuf>> {
    let dir = data_dir.join(RESTORE_DIR);
    if !dir.join(DB_FILE).exists() {
        return Ok(None);
    }
    let keep = data_dir.join(format!("pre-restore-{}", Utc::now().format("%Y%m%d-%H%M%S")));
    std::fs::create_dir_all(&keep)?;
    for name in std::iter::once(DB_FILE).chain(FILES.iter().copied()).chain(["directory.db-wal", "directory.db-shm"]) {
        let cur = data_dir.join(name);
        if cur.exists() {
            std::fs::rename(&cur, keep.join(name))?;
        }
    }
    for name in std::iter::once(DB_FILE).chain(FILES.iter().copied()) {
        let staged = dir.join(name);
        if staged.exists() {
            std::fs::rename(&staged, data_dir.join(name))?;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(Some(keep))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encrypted_and_plain() {
        let dir = std::env::temp_dir().join(format!("od-bk-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join(DB_FILE)).unwrap();
        db.meta_set("domain", "corp.example.com").unwrap();
        std::fs::write(dir.join("master.key"), [9u8; 32]).unwrap();

        let enc = create(&db, &dir, Some("correct horse battery")).unwrap();
        assert!(read_header(&enc).unwrap().encrypted);
        assert!(open(&enc, Some("wrong passphrase!!")).is_err());
        assert!(open(&enc, None).is_err());
        let (_, files) = open(&enc, Some("correct horse battery")).unwrap();
        assert_eq!(files["master.key"], vec![9u8; 32]);
        let v = verify(&enc, Some("correct horse battery")).unwrap();
        assert_eq!(v.header.domain.as_deref(), Some("corp.example.com"));

        // Tampering with the header is detected.
        let mut bad = enc.clone();
        let pos = bad.windows(4).position(|w| w == b"corp").unwrap();
        bad[pos] = b'k';
        assert!(open(&bad, Some("correct horse battery")).is_err());

        let plain = create(&db, &dir, None).unwrap();
        assert!(!read_header(&plain).unwrap().encrypted);
        verify(&plain, None).unwrap();
        assert!(create(&db, &dir, Some("short")).is_err());

        // Stage and apply.
        let target = dir.join("restore-target");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join(DB_FILE), b"old").unwrap();
        stage_restore(&target, &enc, Some("correct horse battery")).unwrap();
        let kept = apply_pending(&target).unwrap().unwrap();
        assert_eq!(std::fs::read(kept.join(DB_FILE)).unwrap(), b"old");
        assert_eq!(std::fs::read(target.join("master.key")).unwrap(), vec![9u8; 32]);
        assert!(apply_pending(&target).unwrap().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
