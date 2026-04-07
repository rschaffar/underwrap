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

use anyhow::Result;
use underwrap_protocol::config::StorageConfig;

/// The storage backend trait.
///
/// Implementations must be safe to share across threads (`Send + Sync`)
/// because the daemon serves concurrent connections.
pub trait StorageBackend: Send + Sync {
    /// Read secret material by id. Returns `Ok(None)` if the secret
    /// does not exist yet.
    fn read(&self, secret_id: &str) -> Result<Option<String>>;

    /// Write (create or replace) secret material. Implementations should
    /// write atomically where possible.
    fn write(&self, secret_id: &str, material: &str) -> Result<()>;

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
