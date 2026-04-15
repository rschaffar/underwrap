//! Storage backend trait and built-in implementations for underwrap.
//!
//! Built-in backends:
//! - [`PlainBackend`]: plaintext files in a directory
//! - [`EncryptedBackend`]: ChaCha20-Poly1305 encrypted files with a master key
//!
//! The [`StorageBackend`] trait is intentionally simple so that future
//! backends (e.g. exec-based providers for libsecret, 1Password, Vault)
//! can implement it without coupling to filesystem details.

mod encrypted;
mod path;
mod plain;
mod remote;

pub use encrypted::EncryptedBackend;
pub use plain::PlainBackend;
pub use remote::RemoteBackend;
pub use remote::RemoteBackend as UnixSocketBackend;

use anyhow::{Result, bail};
use underwrap_protocol::config::{BackendMode, BackendSocketConfig, StorageConfig};
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

/// Open a storage backend from a [`StorageConfig`].
pub fn open(config: &StorageConfig) -> Result<Box<dyn StorageBackend>> {
    match config {
        StorageConfig::Plain { dir } => Ok(Box::new(PlainBackend::open(dir)?)),
        StorageConfig::Encrypted { dir, key_file } => {
            Ok(Box::new(EncryptedBackend::open(dir, key_file)?))
        }
        StorageConfig::Remote { socket } => Ok(Box::new(RemoteBackend::open(socket)?)),
    }
}

/// Open the frontend backend client from daemon backend configuration.
pub fn open_backend_client(config: &BackendSocketConfig) -> Result<Box<dyn StorageBackend>> {
    match config.mode {
        BackendMode::UnixSocket => Ok(Box::new(UnixSocketBackend::open(&config.socket)?)),
        BackendMode::Service => {
            let url = config.url.as_deref().unwrap_or("<missing>");
            bail!("backend service mode is not implemented yet (configured url: {url})")
        }
    }
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
    fn open_dispatches_to_remote_backend() {
        let backend = open(&StorageConfig::Remote {
            socket: "/tmp/nonexistent-underwrap-backend.sock".into(),
        })
        .unwrap();
        let err = backend.read("github/token").unwrap_err();
        assert!(err.to_string().contains("connecting to backend at"));
    }

    #[test]
    fn open_backend_client_dispatches_to_unix_socket_mode() {
        let backend = open_backend_client(&BackendSocketConfig {
            mode: BackendMode::UnixSocket,
            socket: "/tmp/nonexistent-underwrap-backend.sock".into(),
            url: None,
        })
        .unwrap();
        let err = backend.read("github/token").unwrap_err();
        assert!(err.to_string().contains("connecting to backend at"));
    }

    #[test]
    fn open_backend_client_rejects_unimplemented_service_mode() {
        let err = match open_backend_client(&BackendSocketConfig {
            mode: BackendMode::Service,
            socket: "/tmp/ignored.sock".into(),
            url: Some("https://backend.example.internal".to_string()),
        }) {
            Ok(_) => panic!("expected service backend mode to be rejected"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("backend service mode is not implemented yet")
        );
    }
}
