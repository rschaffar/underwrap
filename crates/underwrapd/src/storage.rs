//! Secret storage backends.
//!
//! - Plain: files in a directory, owned by daemon user.
//! - Encrypted: files encrypted with ChaCha20-Poly1305 using a master key.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use underwrap_protocol::config::StorageConfig;

/// Encryption key (32 bytes for ChaCha20-Poly1305).
type MasterKey = [u8; 32];

pub struct Store {
    dir: PathBuf,
    /// None = plain backend. Some = encrypted backend.
    key: Option<MasterKey>,
}

impl Store {
    pub fn open(config: &StorageConfig) -> Result<Self> {
        match config {
            StorageConfig::Plain { dir } => {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("creating storage dir {}", dir.display()))?;
                let canonical = dir
                    .canonicalize()
                    .with_context(|| format!("canonicalizing storage dir {}", dir.display()))?;
                Ok(Self {
                    dir: canonical,
                    key: None,
                })
            }
            StorageConfig::Encrypted { dir, key_file } => {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("creating storage dir {}", dir.display()))?;
                let canonical = dir
                    .canonicalize()
                    .with_context(|| format!("canonicalizing storage dir {}", dir.display()))?;

                // Verify key file permissions (should be 0600 or 0400).
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
                    dir: canonical,
                    key: Some(key),
                })
            }
        }
    }

    /// Read secret material by id.
    pub fn read(&self, secret_id: &str) -> Result<Option<String>> {
        let path = self.validated_path(secret_id)?;
        let raw = match std::fs::read(&path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading secret {secret_id}")),
        };

        let plaintext = match &self.key {
            None => raw,
            Some(key) => {
                decrypt(key, &raw).with_context(|| format!("decrypting secret {secret_id}"))?
            }
        };

        String::from_utf8(plaintext)
            .map(|s| Some(s))
            .with_context(|| format!("secret {secret_id} is not valid UTF-8"))
    }

    /// Write (create or replace) secret material atomically.
    pub fn write(&self, secret_id: &str, material: &str) -> Result<()> {
        let path = self.validated_path(secret_id)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let data = match &self.key {
            None => material.as_bytes().to_vec(),
            Some(key) => encrypt(key, material.as_bytes())?,
        };

        // Atomic write: temp file + rename.
        let parent = path.parent().unwrap_or(&self.dir);
        let mut tmp_path = parent.to_path_buf();
        tmp_path.push(format!(
            ".tmp.{}.{}",
            std::process::id(),
            secret_id.replace('/', "_")
        ));

        std::fs::write(&tmp_path, &data)
            .with_context(|| format!("writing temp file for secret {secret_id}"))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
        }

        std::fs::rename(&tmp_path, &path).with_context(|| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("renaming temp file for secret {secret_id}")
        })?;

        Ok(())
    }

    /// Validate and resolve a secret_id to a safe filesystem path.
    fn validated_path(&self, secret_id: &str) -> Result<PathBuf> {
        if secret_id.contains('\0') {
            bail!("secret id must not contain null bytes: {secret_id:?}");
        }
        if secret_id.is_empty() {
            bail!("secret id must not be empty");
        }
        if secret_id.starts_with('/') {
            bail!("secret id must not be an absolute path: {secret_id:?}");
        }
        for component in secret_id.split('/') {
            if component.is_empty() {
                bail!("secret id must not contain empty path segments: {secret_id:?}");
            }
            if component == "." || component == ".." {
                bail!("secret id must not contain '.' or '..': {secret_id:?}");
            }
        }

        let candidate = self.dir.join(secret_id);

        if candidate.exists() {
            let canonical = candidate
                .canonicalize()
                .with_context(|| format!("canonicalizing secret path for {secret_id:?}"))?;
            if !canonical.starts_with(&self.dir) {
                bail!(
                    "secret path escapes storage directory: {secret_id:?} -> {}",
                    canonical.display()
                );
            }
        }

        Ok(candidate)
    }
}

// ---------------------------------------------------------------------------
// Key loading
// ---------------------------------------------------------------------------

fn load_master_key(path: &std::path::Path) -> Result<MasterKey> {
    let raw = std::fs::read(path)
        .with_context(|| format!("reading master key from {}", path.display()))?;

    // Try raw 32 bytes first.
    if raw.len() == 32 {
        let mut key = [0u8; 32];
        key.copy_from_slice(&raw);
        return Ok(key);
    }

    // Try hex-encoded (64 chars + optional newline).
    let hex_str = std::str::from_utf8(&raw)
        .context("master key file is not valid UTF-8")?
        .trim();
    if hex_str.len() == 64 {
        let mut key = [0u8; 32];
        for (i, chunk) in hex_str.as_bytes().chunks(2).enumerate() {
            let s = std::str::from_utf8(chunk).unwrap();
            key[i] = u8::from_str_radix(s, 16)
                .with_context(|| format!("invalid hex in master key at position {}", i * 2))?;
        }
        return Ok(key);
    }

    bail!(
        "master key must be 32 raw bytes or 64 hex characters, got {} bytes",
        raw.len()
    );
}

// ---------------------------------------------------------------------------
// Encryption: ChaCha20-Poly1305
// Wire format: [12-byte nonce][ciphertext+tag]
// ---------------------------------------------------------------------------

fn encrypt(key: &MasterKey, plaintext: &[u8]) -> Result<Vec<u8>> {
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

fn decrypt(key: &MasterKey, data: &[u8]) -> Result<Vec<u8>> {
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

    fn test_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::TempDir::new().unwrap();
        let config = StorageConfig::Plain {
            dir: dir.path().to_path_buf(),
        };
        let store = Store::open(&config).unwrap();
        (dir, store)
    }

    fn test_encrypted_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::TempDir::new().unwrap();
        let key_path = dir.path().join("master.key");

        // Generate a random key.
        let key: [u8; 32] = rand::random();
        std::fs::write(&key_path, &key).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }

        let config = StorageConfig::Encrypted {
            dir: dir.path().join("secrets"),
            key_file: key_path,
        };
        let store = Store::open(&config).unwrap();
        (dir, store)
    }

    // --- Plain backend tests ---

    #[test]
    fn read_nonexistent_returns_none() {
        let (_dir, store) = test_store();
        assert!(store.read("nope").unwrap().is_none());
    }

    #[test]
    fn write_then_read() {
        let (_dir, store) = test_store();
        store.write("my/secret", "hello").unwrap();
        assert_eq!(store.read("my/secret").unwrap().unwrap(), "hello");
    }

    #[test]
    fn overwrite() {
        let (_dir, store) = test_store();
        store.write("tok", "v1").unwrap();
        store.write("tok", "v2").unwrap();
        assert_eq!(store.read("tok").unwrap().unwrap(), "v2");
    }

    #[test]
    fn deep_hierarchy() {
        let (_dir, store) = test_store();
        store.write("a/b/c/d/e", "deep").unwrap();
        assert_eq!(store.read("a/b/c/d/e").unwrap().unwrap(), "deep");
    }

    // --- Path traversal rejection ---

    #[test]
    fn rejects_dotdot() {
        let (_dir, store) = test_store();
        assert!(store.write("../etc/passwd", "pwned").is_err());
    }

    #[test]
    fn rejects_absolute_path() {
        let (_dir, store) = test_store();
        assert!(store.write("/etc/shadow", "pwned").is_err());
    }

    #[test]
    fn rejects_dot_component() {
        let (_dir, store) = test_store();
        assert!(store.write("./secret", "data").is_err());
    }

    #[test]
    fn rejects_empty_segments() {
        let (_dir, store) = test_store();
        assert!(store.write("a//b", "data").is_err());
    }

    #[test]
    fn rejects_empty_id() {
        let (_dir, store) = test_store();
        assert!(store.write("", "data").is_err());
    }

    #[test]
    fn rejects_null_bytes() {
        let (_dir, store) = test_store();
        assert!(store.write("secret\0id", "data").is_err());
    }

    #[test]
    fn empty_material() {
        let (_dir, store) = test_store();
        store.write("empty", "").unwrap();
        assert_eq!(store.read("empty").unwrap().unwrap(), "");
    }

    #[test]
    fn atomic_write_leaves_no_temp_files() {
        let (dir, store) = test_store();
        store.write("tok", "val").unwrap();
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().to_string();
            assert!(!name.starts_with(".tmp."), "temp file left behind: {name}");
        }
    }

    // --- Encrypted backend tests ---

    #[test]
    fn encrypted_write_then_read() {
        let (_dir, store) = test_encrypted_store();
        store.write("secret/tok", "encrypted_value").unwrap();
        assert_eq!(
            store.read("secret/tok").unwrap().unwrap(),
            "encrypted_value"
        );
    }

    #[test]
    fn encrypted_file_is_not_plaintext() {
        let (dir, store) = test_encrypted_store();
        store.write("tok", "my_secret_data").unwrap();

        // Read the raw file — it should NOT contain the plaintext.
        let secrets_dir = dir.path().join("secrets");
        let raw = std::fs::read(secrets_dir.join("tok")).unwrap();
        let raw_str = String::from_utf8_lossy(&raw);
        assert!(
            !raw_str.contains("my_secret_data"),
            "plaintext found in encrypted file!"
        );
        // Should be longer than plaintext (nonce + tag overhead).
        assert!(raw.len() > "my_secret_data".len());
    }

    #[test]
    fn encrypted_overwrite() {
        let (_dir, store) = test_encrypted_store();
        store.write("tok", "v1").unwrap();
        store.write("tok", "v2").unwrap();
        assert_eq!(store.read("tok").unwrap().unwrap(), "v2");
    }

    #[test]
    fn encrypted_empty_material() {
        let (_dir, store) = test_encrypted_store();
        store.write("empty", "").unwrap();
        assert_eq!(store.read("empty").unwrap().unwrap(), "");
    }

    #[test]
    fn encrypted_wrong_key_fails() {
        let (dir, store) = test_encrypted_store();
        store.write("tok", "secret").unwrap();

        // Open a new store with a different key.
        let key_path = dir.path().join("other.key");
        let key: [u8; 32] = rand::random();
        std::fs::write(&key_path, &key).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }

        let config = StorageConfig::Encrypted {
            dir: dir.path().join("secrets"),
            key_file: key_path,
        };
        let store2 = Store::open(&config).unwrap();

        // Reading with wrong key should fail.
        let result = store2.read("tok");
        assert!(result.is_err(), "expected decryption error");
    }

    #[test]
    fn hex_key_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let key_path = dir.path().join("master.hex");

        // Write hex-encoded key.
        let key: [u8; 32] = rand::random();
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        std::fs::write(&key_path, format!("{hex}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }

        let config = StorageConfig::Encrypted {
            dir: dir.path().join("secrets"),
            key_file: key_path,
        };
        let store = Store::open(&config).unwrap();
        store.write("tok", "hex_key_test").unwrap();
        assert_eq!(store.read("tok").unwrap().unwrap(), "hex_key_test");
    }

    #[test]
    fn large_encrypted_secret() {
        let (_dir, store) = test_encrypted_store();
        let big = "x".repeat(32_000);
        store.write("big", &big).unwrap();
        assert_eq!(store.read("big").unwrap().unwrap(), big);
    }
}
