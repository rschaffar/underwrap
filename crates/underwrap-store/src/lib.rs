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

pub use encrypted::EncryptedBackend;
pub use plain::PlainBackend;

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
}

/// Open a storage backend from a [`StorageConfig`].
pub fn open(config: &StorageConfig) -> Result<Box<dyn StorageBackend>> {
    match config {
        StorageConfig::Plain { dir } => Ok(Box::new(PlainBackend::open(dir)?)),
        StorageConfig::Encrypted { dir, key_file } => {
            Ok(Box::new(EncryptedBackend::open(dir, key_file)?))
        }
    }
}
