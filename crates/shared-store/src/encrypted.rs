//! Encrypted file-based storage backend (ChaCha20-Poly1305).
//!
//! The 32-byte master key lives in process memory only — it is never
//! written to disk. Callers obtain a key either by generating fresh
//! random bytes (on store creation) or by parsing an operator-supplied
//! hex string (on unlock). See [`crate::parse_hex_key`].

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::StorageBackend;
use crate::path::{
    atomic_write, cleanup_empty_parents, list_files_recursive, prepare_dir, validated_path,
};
use underwrap_protocol::message::SecretBytes;

/// Master key — zeroed from memory on drop.
#[derive(Zeroize, ZeroizeOnDrop)]
struct MasterKey([u8; 32]);

/// Files encrypted with ChaCha20-Poly1305 using a master key.
///
/// Wire format per file: `[12-byte nonce][ciphertext + 16-byte tag]`
pub struct EncryptedBackend {
    dir: PathBuf,
    key: MasterKey,
}

impl EncryptedBackend {
    /// Open the backend against `dir` using the in-memory `key`. The
    /// caller's `key` is moved into the backend and zeroized on drop.
    pub fn open_with_key(dir: &Path, key: [u8; 32]) -> Result<Self> {
        Ok(Self {
            dir: prepare_dir(dir)?,
            key: MasterKey(key),
        })
    }
}

impl StorageBackend for EncryptedBackend {
    fn read(&self, secret_id: &str) -> Result<Option<SecretBytes>> {
        let path = validated_path(&self.dir, secret_id)?;
        let raw = match std::fs::read(&path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading secret {secret_id}")),
        };
        let plaintext =
            decrypt(&self.key.0, &raw).with_context(|| format!("decrypting secret {secret_id}"))?;
        Ok(Some(plaintext.into()))
    }

    fn write(&self, secret_id: &str, material: &[u8]) -> Result<()> {
        let path = validated_path(&self.dir, secret_id)?;
        let data = encrypt(&self.key.0, material)?;
        atomic_write(&path, &self.dir, secret_id, &data)
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        list_files_recursive(&self.dir, prefix)
    }

    fn delete(&self, secret_id: &str) -> Result<bool> {
        let path = validated_path(&self.dir, secret_id)?;
        match std::fs::remove_file(&path) {
            Ok(()) => {
                cleanup_empty_parents(&path, &self.dir);
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e).with_context(|| format!("deleting secret {secret_id}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Encryption
// ---------------------------------------------------------------------------

fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
    use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
    use chacha20poly1305::{AeadCore, ChaCha20Poly1305};

    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>> {
    use chacha20poly1305::ChaCha20Poly1305;
    use chacha20poly1305::aead::{Aead, KeyInit};

    if data.len() < 12 {
        bail!("encrypted data too short ({} bytes)", data.len());
    }

    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = chacha20poly1305::Nonce::from_slice(nonce_bytes);
    let cipher = ChaCha20Poly1305::new(key.into());

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed (wrong key or corrupted data): {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_backend() -> (tempfile::TempDir, EncryptedBackend) {
        let dir = tempfile::TempDir::new().unwrap();
        let key: [u8; 32] = rand::random();
        let backend = EncryptedBackend::open_with_key(&dir.path().join("secrets"), key).unwrap();
        (dir, backend)
    }

    #[test]
    fn write_then_read() {
        let (_dir, b) = test_backend();
        b.write("secret/tok", b"encrypted_value").unwrap();
        assert_eq!(b.read("secret/tok").unwrap().unwrap(), b"encrypted_value");
    }

    #[test]
    fn file_is_not_plaintext() {
        let (dir, b) = test_backend();
        b.write("tok", b"my_secret_data").unwrap();
        let raw = std::fs::read(dir.path().join("secrets/tok")).unwrap();
        let raw_str = String::from_utf8_lossy(&raw);
        assert!(
            !raw_str.contains("my_secret_data"),
            "plaintext found in encrypted file!"
        );
        assert!(raw.len() > "my_secret_data".len());
    }

    #[test]
    fn overwrite() {
        let (_dir, b) = test_backend();
        b.write("tok", b"v1").unwrap();
        b.write("tok", b"v2").unwrap();
        assert_eq!(b.read("tok").unwrap().unwrap(), b"v2");
    }

    #[test]
    fn empty_material() {
        let (_dir, b) = test_backend();
        b.write("empty", b"").unwrap();
        assert_eq!(b.read("empty").unwrap().unwrap(), b"");
    }

    #[test]
    fn wrong_key_fails() {
        let (dir, b) = test_backend();
        b.write("tok", b"secret").unwrap();

        let other_key: [u8; 32] = rand::random();
        let b2 = EncryptedBackend::open_with_key(&dir.path().join("secrets"), other_key).unwrap();
        assert!(b2.read("tok").is_err());
    }

    #[test]
    fn large_secret() {
        let (_dir, b) = test_backend();
        let big = "x".repeat(32_000);
        b.write("big", big.as_bytes()).unwrap();
        assert_eq!(b.read("big").unwrap().unwrap(), big);
    }

    #[test]
    fn list_and_delete() {
        let (_dir, b) = test_backend();
        b.write("a/item1", b"v1").unwrap();
        b.write("a/item2", b"v2").unwrap();
        b.write("b/item3", b"v3").unwrap();

        let mut ids = b.list("a").unwrap();
        ids.sort();
        assert_eq!(ids, vec!["a/item1", "a/item2"]);

        assert!(b.delete("a/item1").unwrap());
        assert!(!b.delete("nonexistent").unwrap());

        let ids = b.list("a").unwrap();
        assert_eq!(ids, vec!["a/item2"]);

        // Deleted item can't be read.
        assert!(b.read("a/item1").unwrap().is_none());
    }
}
