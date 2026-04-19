//! Encrypted file-based storage backend (ChaCha20-Poly1305).

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
    pub fn open(dir: &Path, key_file: &Path) -> Result<Self> {
        // Verify key file permissions.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(key_file)
                .with_context(|| format!("reading key file {}", key_file.display()))?;
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                bail!(
                    "key file {} has insecure permissions {:#o} (must not be group/world readable)",
                    key_file.display(),
                    mode
                );
            }
        }

        let key = load_master_key(key_file)?;
        Ok(Self {
            dir: prepare_dir(dir)?,
            key,
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
// Key loading
// ---------------------------------------------------------------------------

fn load_master_key(path: &Path) -> Result<MasterKey> {
    let mut raw = std::fs::read(path)
        .with_context(|| format!("reading master key from {}", path.display()))?;

    if raw.len() == 32 {
        let mut key = [0u8; 32];
        key.copy_from_slice(&raw);
        raw.zeroize();
        return Ok(MasterKey(key));
    }

    let hex_str = std::str::from_utf8(&raw)
        .context("master key file is not valid UTF-8")?
        .trim();
    if hex_str.len() == 64 {
        let mut key = [0u8; 32];
        for (i, byte) in key.iter_mut().enumerate() {
            let s = hex_str
                .get(i * 2..i * 2 + 2)
                .with_context(|| format!("invalid hex in master key at position {}", i * 2))?;
            *byte = u8::from_str_radix(s, 16)
                .with_context(|| format!("invalid hex in master key at position {}", i * 2))?;
        }
        raw.zeroize();
        return Ok(MasterKey(key));
    }

    bail!(
        "master key must be 32 raw bytes or 64 hex characters, got {} bytes",
        raw.len()
    );
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
        let key_path = dir.path().join("master.key");
        let key: [u8; 32] = rand::random();
        std::fs::write(&key_path, key).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let backend = EncryptedBackend::open(&dir.path().join("secrets"), &key_path).unwrap();
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

        let key_path = dir.path().join("other.key");
        let key: [u8; 32] = rand::random();
        std::fs::write(&key_path, key).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let b2 = EncryptedBackend::open(&dir.path().join("secrets"), &key_path).unwrap();
        assert!(b2.read("tok").is_err());
    }

    #[test]
    fn hex_key_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let key_path = dir.path().join("master.hex");
        let key: [u8; 32] = rand::random();
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        std::fs::write(&key_path, format!("{hex}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let b = EncryptedBackend::open(&dir.path().join("secrets"), &key_path).unwrap();
        b.write("tok", b"hex_key_test").unwrap();
        assert_eq!(b.read("tok").unwrap().unwrap(), b"hex_key_test");
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
