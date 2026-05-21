//! Storage backend trait and built-in implementations for underwrap.
//!
//! Built-in backends used inside `underwrap-backendd`:
//! - [`PlainBackend`]: plaintext files in a directory
//! - [`EncryptedBackend`]: ChaCha20-Poly1305 encrypted files with a master key
//!
//! The frontend daemon `underwrapd` no longer goes through this trait
//! to reach the central backend. It speaks the resource-vocabulary
//! runtime API via `frontend-underwrapd::backend_client::BackendClient`
//! and never holds a `StorageBackend` itself.

mod encrypted;
mod path;
mod plain;
mod resource_blob;

pub use encrypted::EncryptedBackend;
pub use plain::PlainBackend;
pub use resource_blob::{
    blob_from_resource_entries, delete_resource_blob, expand_blob_to_entries, load_resource_blob,
    save_resource_blob,
};

use anyhow::{Context, Result};
use std::collections::HashMap;
use underwrap_protocol::config::StorageConfig;
use underwrap_protocol::message::SecretBytes;

/// The storage backend trait.
///
/// Implementations must be safe to share across threads (`Send + Sync`)
/// because the backend serves concurrent connections.
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

    /// Read every leaf under a set of prefixes.
    ///
    /// The default implementation loops through `list` + `read`.
    fn read_prefixes(&self, prefixes: &[String]) -> Result<HashMap<String, SecretBytes>> {
        let mut out = HashMap::new();
        for prefix in prefixes {
            for id in self
                .list(prefix)
                .with_context(|| format!("listing bundle prefix '{prefix}'"))?
            {
                if let Some(material) = self
                    .read(&id)
                    .with_context(|| format!("reading bundle id '{id}'"))?
                {
                    out.insert(id, material);
                }
            }
        }
        Ok(out)
    }
}

/// Open a [`StorageBackend`] that does not require additional unlock
/// material. This is currently limited to [`StorageConfig::Plain`].
///
/// Encrypted stores need a 32-byte master key supplied by the caller
/// (held only in process memory) and must therefore be opened
/// explicitly via [`EncryptedBackend::open_with_key`]. The dispatcher
/// here returns an explanatory error if asked to open an encrypted
/// store so a forgotten code path fails loudly instead of silently
/// constructing an unusable backend.
pub fn open(config: &StorageConfig) -> Result<Box<dyn StorageBackend>> {
    match config {
        StorageConfig::Plain { dir } => Ok(Box::new(PlainBackend::open(dir)?)),
        StorageConfig::Encrypted { .. } => {
            anyhow::bail!(
                "encrypted stores must be opened with an in-memory master key via \
                 EncryptedBackend::open_with_key — they cannot be opened by this generic dispatcher",
            )
        }
    }
}

/// Parse exactly 64 hex characters into a 32-byte master key. Leading
/// or trailing whitespace is trimmed. Used by the admin unlock and
/// rotate-key flows to convert an operator-supplied hex string into
/// the byte form consumed by [`EncryptedBackend::open_with_key`].
pub fn parse_hex_key(s: &str) -> Result<[u8; 32]> {
    let trimmed = s.trim();
    anyhow::ensure!(
        trimmed.len() == 64,
        "master key must be exactly 64 hex characters (got {} characters)",
        trimmed.chars().count(),
    );
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        let slice = trimmed
            .get(i * 2..i * 2 + 2)
            .with_context(|| format!("master key hex truncated at position {}", i * 2))?;
        *byte = u8::from_str_radix(slice, 16)
            .with_context(|| format!("invalid hex in master key at position {}", i * 2))?;
    }
    Ok(key)
}

/// Reserved secret id used by encrypted stores as an unlock canary. On
/// store creation the backend writes [`ENCRYPTED_CANARY_PLAINTEXT`] at
/// this id under the chosen master key. Unlock verifies the operator's
/// key by attempting to decrypt this entry — wrong key fails the
/// AEAD tag check, so the operator cannot install an invalid key by
/// accident. The canary is filtered from `list("")` results so it
/// doesn't show up as a stray resource in the admin browser.
pub const ENCRYPTED_CANARY_ID: &str = "__underwrap__/canary";

/// Fixed plaintext written to [`ENCRYPTED_CANARY_ID`]. Decrypting it
/// to exactly this byte string is what makes an unlock successful.
pub const ENCRYPTED_CANARY_PLAINTEXT: &[u8] = b"underwrap-encrypted-store-canary-v1";

#[cfg(test)]
mod tests {
    use super::*;

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
    fn open_refuses_encrypted_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let result = open(&StorageConfig::Encrypted {
            dir: dir.path().join("encrypted"),
        });
        match result {
            Ok(_) => panic!("encrypted configs must be refused by the generic dispatcher"),
            Err(err) => assert!(
                err.to_string().contains("EncryptedBackend::open_with_key"),
                "unexpected error: {err}",
            ),
        }
    }

    #[test]
    fn parse_hex_key_accepts_canonical_lowercase() {
        let hex: String = (0u8..32).map(|b| format!("{b:02x}")).collect();
        let bytes = parse_hex_key(&hex).unwrap();
        for (i, b) in bytes.iter().enumerate() {
            assert_eq!(*b, i as u8);
        }
    }

    #[test]
    fn parse_hex_key_accepts_uppercase_and_whitespace() {
        let hex: String = (0u8..32).map(|b| format!("{b:02X}")).collect();
        let padded = format!("\n  {hex}  \n");
        assert!(parse_hex_key(&padded).is_ok());
    }

    #[test]
    fn parse_hex_key_rejects_wrong_length() {
        let err = parse_hex_key("deadbeef").unwrap_err();
        assert!(
            err.to_string().contains("exactly 64 hex characters"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_hex_key_rejects_non_hex() {
        let bad = "z".repeat(64);
        assert!(parse_hex_key(&bad).is_err());
    }
}
