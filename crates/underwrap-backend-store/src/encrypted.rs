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
use crate::path::{atomic_write, cleanup_empty_parents, list_files_recursive, validated_path};
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
    /// Create a new encrypted backend directory exclusively. Existing files,
    /// directories, and symlinks are rejected rather than reused.
    pub fn create_with_key(dir: &Path, key: [u8; 32]) -> Result<Self> {
        let key = MasterKey(key);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(dir)
                .with_context(|| format!("creating encrypted storage dir {}", dir.display()))?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir(dir)
            .with_context(|| format!("creating encrypted storage dir {}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(error) =
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            {
                let _ = std::fs::remove_dir(dir);
                return Err(error)
                    .with_context(|| format!("securing encrypted storage dir {}", dir.display()));
            }
        }

        match Self::open_existing(dir, key) {
            Ok(backend) => Ok(backend),
            Err(error) => {
                // This call created the directory exclusively and has not
                // written ciphertext yet, so only that new empty directory is
                // eligible for cleanup.
                let _ = std::fs::remove_dir(dir);
                Err(error)
            }
        }
    }

    /// Open an existing encrypted backend without creating or repairing its
    /// directory. The fixed-size key is copied into the backend and zeroized on
    /// drop; callers should wipe their temporary array immediately afterward.
    pub fn open_existing_with_key(dir: &Path, key: [u8; 32]) -> Result<Self> {
        Self::open_existing(dir, MasterKey(key))
    }

    fn open_existing(dir: &Path, key: MasterKey) -> Result<Self> {
        let metadata = std::fs::symlink_metadata(dir)
            .with_context(|| format!("inspecting encrypted storage dir {}", dir.display()))?;
        anyhow::ensure!(
            metadata.file_type().is_dir() && !metadata.file_type().is_symlink(),
            "encrypted storage path is not a directory: {}",
            dir.display(),
        );
        Ok(Self {
            dir: dir.canonicalize().with_context(|| {
                format!("canonicalizing encrypted storage dir {}", dir.display())
            })?,
            key,
        })
    }
}

fn ensure_private_parent_directories(base: &Path, parent: &Path) -> Result<()> {
    let relative = parent
        .strip_prefix(base)
        .context("encrypted secret parent escaped storage root")?;
    let mut current = base.to_path_buf();
    for component in relative.components() {
        current.push(component);
        #[cfg(unix)]
        let created = {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(&current)
        };
        #[cfg(not(unix))]
        let created = std::fs::create_dir(&current);
        match created {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = std::fs::symlink_metadata(&current)?;
                anyhow::ensure!(
                    metadata.file_type().is_dir() && !metadata.file_type().is_symlink(),
                    "encrypted storage parent is not a directory: {}",
                    current.display(),
                );
            }
            Err(error) => return Err(error.into()),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&current, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
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
        ensure_private_parent_directories(&self.dir, path.parent().unwrap_or(&self.dir))?;
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
