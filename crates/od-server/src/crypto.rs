//! Password hashing, secret sealing and token helpers.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow!("hash password: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

/// A random URL-safe token with `bytes` bytes of entropy.
pub fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// A short human-friendly enrollment code like `K7QM-2XPA-9DWE-HT4C`.
pub fn enrollment_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    let mut out = String::new();
    for i in 0..16 {
        if i > 0 && i % 4 == 0 {
            out.push('-');
        }
        out.push(ALPHABET[(rng.next_u32() as usize) % ALPHABET.len()] as char);
    }
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// Constant-time string comparison for secrets.
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Encrypts user passwords at rest so the Endpoint Agent can set the same
/// password on local accounts (Windows Home, macOS, Linux). The key lives in
/// the server's data directory and never leaves the Management Server.
#[derive(Clone)]
pub struct Sealer {
    cipher: XChaCha20Poly1305,
}

impl Sealer {
    pub fn load_or_create(path: &Path) -> Result<Sealer> {
        let key = if path.exists() {
            let raw = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
            if raw.len() != 32 {
                return Err(anyhow!("{} is corrupt (expected 32 bytes)", path.display()));
            }
            raw
        } else {
            let mut k = vec![0u8; 32];
            rand::thread_rng().fill_bytes(&mut k);
            write_private(path, &k)?;
            k
        };
        Ok(Sealer { cipher: XChaCha20Poly1305::new_from_slice(&key).map_err(|e| anyhow!("{e}"))? })
    }

    pub fn from_key(key: [u8; 32]) -> Sealer {
        Sealer { cipher: XChaCha20Poly1305::new_from_slice(&key).expect("32-byte key") }
    }

    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0u8; 24];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ct = self.cipher.encrypt(XNonce::from_slice(&nonce), plaintext).map_err(|e| anyhow!("seal: {e}"))?;
        let mut out = nonce.to_vec();
        out.extend(ct);
        Ok(out)
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>> {
        if sealed.len() < 24 {
            return Err(anyhow!("sealed value too short"));
        }
        let (nonce, ct) = sealed.split_at(24);
        self.cipher.decrypt(XNonce::from_slice(nonce), ct).map_err(|e| anyhow!("open: {e}"))
    }
}

/// Write a file readable only by the owner (on Unix).
pub fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    std::fs::write(path, data).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let h = hash_password("correct horse").unwrap();
        assert!(verify_password("correct horse", &h));
        assert!(!verify_password("wrong", &h));
    }

    #[test]
    fn seal_roundtrip() {
        let s = Sealer::from_key([7u8; 32]);
        let sealed = s.seal(b"hunter2").unwrap();
        assert_eq!(s.open(&sealed).unwrap(), b"hunter2");
        assert!(Sealer::from_key([8u8; 32]).open(&sealed).is_err());
    }

    #[test]
    fn enrollment_code_shape() {
        let c = enrollment_code();
        assert_eq!(c.len(), 19);
        assert_eq!(c.matches('-').count(), 3);
    }
}
