//! Storage backend trait and built-in implementations for underwrap.
//!
//! Built-in backends used inside `underwrap-backend`:
//! - [`PlainBackend`]: plaintext files in a directory
//! - [`EncryptedBackend`]: ChaCha20-Poly1305 encrypted files with a master key
//!
//! Only `underwrap-backend` owns a `StorageBackend`. The frontend daemon uses
//! the resource-oriented HTTPS runtime API and never receives this storage
//! interface. Backend migration inventories logical ids and streams one value
//! or verification pair at a time through this trait so a backend-kind change
//! does not change the stored logical bytes or retain a whole-store plaintext
//! snapshot.

mod encrypted;
mod path;
mod plain;
mod resource_blob;

pub use encrypted::EncryptedBackend;
pub use plain::PlainBackend;
pub use resource_blob::{
    delete_resource_blob, expand_blob_to_entries, load_resource_blob, save_resource_blob,
};

use std::path::Path;

use anyhow::{Context, Result};
use underwrap_protocol::config::StorageConfig;
use underwrap_protocol::message::SecretBytes;

/// The storage backend trait.
///
/// Implementations must be safe to share across threads (`Send + Sync`)
/// because the backend serves concurrent connections. The trait provides no
/// transaction, lease, migration, publication, or secure-erasure guarantee;
/// `underwrap-backend` owns those lifecycle rules.
///
/// [`StorageBackend::reopen_at`] is the sole path-aware lifecycle capability.
/// It reopens an already-existing generation with the same backend kind after
/// a checked rename, without creating or repairing storage.
/// An encrypted implementation may copy only its own installed in-memory key
/// into the returned backend, must keep that copy internal, and must zeroize it
/// when opening fails or the returned backend is dropped.
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

    /// Reopen this backend kind over an already-existing generation root.
    ///
    /// This narrow capability exists so lifecycle code can keep a renamed
    /// source generation authoritative without receiving backend-specific key
    /// bytes. Implementations must reject missing, symlink, or non-directory
    /// roots and must not create, repair, convert, or otherwise mutate them.
    fn reopen_at(&self, dir: &Path) -> Result<Box<dyn StorageBackend>>;
}

/// Open a [`StorageBackend`] that does not require additional unlock
/// material. This is currently limited to [`StorageConfig::Plain`].
///
/// Encrypted stores need a 32-byte master key supplied by the caller
/// (held only in process memory) and must therefore be opened
/// explicitly via [`EncryptedBackend::open_existing_with_key`]. The dispatcher
/// here returns an explanatory error if asked to open an encrypted
/// store so a forgotten code path fails loudly instead of silently
/// constructing an unusable backend.
pub fn open(config: &StorageConfig) -> Result<Box<dyn StorageBackend>> {
    match config {
        StorageConfig::Plain { dir } => Ok(Box::new(PlainBackend::open(dir)?)),
        StorageConfig::Encrypted { .. } => {
            anyhow::bail!(
                "encrypted stores must be opened with an in-memory master key via \
                 EncryptedBackend::open_existing_with_key — they cannot be opened by this generic dispatcher",
            )
        }
    }
}

/// Parse exactly 64 hex characters into a 32-byte master key after trimming
/// surrounding whitespace. Admin unlock applies this tolerant parser to an
/// operator-supplied string. Rotation uses it only to reopen its own generated
/// replacement and does not accept an operator-supplied key.
pub fn parse_hex_key(s: &str) -> Result<[u8; 32]> {
    parse_hex_key_bytes(s.trim(), false)
}

/// Parse the encrypted-create and plain-to-encrypted migration key contract
/// without trimming or normalization. Only one 64-character lowercase
/// hexadecimal string is accepted. Encrypted-to-plain migration accepts no key.
pub fn parse_hex_key_strict(s: &str) -> Result<[u8; 32]> {
    parse_hex_key_bytes(s, true)
}

fn parse_hex_key_bytes(value: &str, lowercase_only: bool) -> Result<[u8; 32]> {
    anyhow::ensure!(
        value.len() == 64,
        "master key must be exactly 64 hex characters (got {} characters)",
        value.chars().count(),
    );
    anyhow::ensure!(
        value.bytes().all(|byte| byte.is_ascii_digit()
            || (!lowercase_only && byte.is_ascii_hexdigit())
            || (b'a'..=b'f').contains(&byte)),
        "master key must contain only {}hexadecimal characters",
        if lowercase_only { "lowercase " } else { "" },
    );
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        let slice = value
            .get(i * 2..i * 2 + 2)
            .with_context(|| format!("master key hex truncated at position {}", i * 2))?;
        *byte = u8::from_str_radix(slice, 16)
            .with_context(|| format!("invalid hex in master key at position {}", i * 2))?;
    }
    Ok(key)
}

/// Reserved key used by encrypted stores as an unlock canary. Encrypted
/// creation writes [`ENCRYPTED_CANARY_PLAINTEXT`] at this id under the submitted
/// master key. Plain-to-encrypted migration writes and verifies it in the
/// target. Encrypted-to-plain migration verifies the source canary and omits it from
/// the plain target. Unlock verifies the operator's key by attempting to
/// decrypt this entry — wrong key fails the AEAD tag check, so the operator
/// cannot install an invalid key by accident. It lives under the reserved control-plane namespace
/// (`underwrap_protocol::control::CONTROL_PREFIX`), so the single
/// `is_control_key` rule hides it from the operator's secret browser
/// alongside the policy / resources / pairs documents.
pub const ENCRYPTED_CANARY_ID: &str = "control/canary";

/// Fixed plaintext written to [`ENCRYPTED_CANARY_ID`]. Decrypting it
/// to exactly this byte string is what makes an unlock successful.
pub const ENCRYPTED_CANARY_PLAINTEXT: &[u8] = b"underwrap-encrypted-store-canary-v1";

#[cfg(test)]
mod tests;
