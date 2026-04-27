//! Storage backend trait and built-in implementations for underwrap.
//!
//! Built-in backends used inside `underwrap-backendd`:
//! - [`PlainBackend`]: plaintext files in a directory
//! - [`EncryptedBackend`]: ChaCha20-Poly1305 encrypted files with a master key
//!
//! The frontend daemon `underwrapd` talks to a central backend over HTTPS
//! via [`ServiceBackend`].

mod encrypted;
mod path;
mod plain;
mod service;

pub use encrypted::EncryptedBackend;
pub use plain::PlainBackend;
pub use service::ServiceBackend;

use anyhow::Result;
use underwrap_protocol::config::{BackendServiceConfig, StorageConfig};
use underwrap_protocol::message::SecretBytes;

/// The storage backend trait.
///
/// Implementations must be safe to share across threads (`Send + Sync`)
/// because the daemon serves concurrent connections.
pub trait StorageBackend: Send + Sync {
    /// Read secret material by id. Returns `Ok(None)` if the secret
    /// does not exist yet.
    fn read(&self, secret_id: &str) -> Result<Option<SecretBytes>>;

    /// Write (create or replace) secret material. Implementations should
    /// write atomically where possible.
    fn write(&self, secret_id: &str, material: &[u8]) -> Result<()>;

    /// List secret IDs under a prefix. Returns IDs relative to the
    /// storage root (not relative to the prefix). Only returns leaf
    /// entries (actual secrets), not intermediate directories.
    /// An empty prefix lists all secrets.
    fn list(&self, prefix: &str) -> Result<Vec<String>>;

    /// Delete a secret by id. Returns `Ok(true)` if the secret existed
    /// and was deleted, `Ok(false)` if it did not exist.
    fn delete(&self, secret_id: &str) -> Result<bool>;
}

/// Open a storage backend from a [`StorageConfig`]. Used by
/// `underwrap-backendd` to open the stores it owns.
pub fn open(config: &StorageConfig) -> Result<Box<dyn StorageBackend>> {
    match config {
        StorageConfig::Plain { dir } => Ok(Box::new(PlainBackend::open(dir)?)),
        StorageConfig::Encrypted { dir, key_file } => {
            Ok(Box::new(EncryptedBackend::open(dir, key_file)?))
        }
    }
}

/// Open the frontend backend client from daemon backend configuration.
/// The frontend only ever talks to a central backend over HTTPS.
pub fn open_backend_client(config: &BackendServiceConfig) -> Result<Box<dyn StorageBackend>> {
    Ok(Box::new(ServiceBackend::open(config)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn open_dispatches_to_plain_backend() {
        let dir = tempfile::TempDir::new().unwrap();
        let backend = open(&StorageConfig::Plain {
            dir: dir.path().join("plain"),
        })
        .unwrap();
        backend.write("github/token", b"ghp_test").unwrap();
        assert_eq!(backend.read("github/token").unwrap().unwrap(), b"ghp_test");
    }

    #[test]
    fn open_dispatches_to_encrypted_backend() {
        let dir = tempfile::TempDir::new().unwrap();
        let key_path = dir.path().join("key");
        std::fs::write(&key_path, [7u8; 32]).unwrap();
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let backend = open(&StorageConfig::Encrypted {
            dir: dir.path().join("encrypted"),
            key_file: key_path,
        })
        .unwrap();
        backend.write("github/token", b"ghp_test").unwrap();
        assert_eq!(backend.read("github/token").unwrap().unwrap(), b"ghp_test");
    }

    #[test]
    fn open_backend_client_rejects_missing_service_tls_settings() {
        let err = match open_backend_client(&BackendServiceConfig {
            url: "https://backend.example.internal".to_string(),
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
        }) {
            Ok(_) => panic!("expected open to fail without TLS material"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("ca_file"));
    }
}
