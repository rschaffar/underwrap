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
mod tests;
