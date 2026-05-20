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
mod resource_blob;
mod service;

pub use encrypted::EncryptedBackend;
pub use plain::PlainBackend;
pub use resource_blob::{
    blob_from_resource_entries, delete_resource_blob, expand_blob_to_entries, load_resource_blob,
    save_resource_blob,
};
pub use service::{ServiceBackend, ServiceStoreLocked};

use anyhow::{Context, Result};
use std::collections::HashMap;
use underwrap_protocol::config::{BackendServiceConfig, StorageConfig};
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::service::{ServiceWrapperRegisterRequest, ServiceWrapperRegisterResponse};

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

    /// Read every leaf under a set of prefixes.
    ///
    /// The default implementation loops through `list` + `read`. The service
    /// backend overrides this so a wrapper invocation can fetch its full
    /// resource bundle in a single HTTPS request.
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

    /// Register the daemon's `(wrapper, resource)` pairs with the
    /// backend. The backend gates auto-creation on the calling client
    /// instance's `auto_create_wrapper_assets` flag and returns either
    /// the list of newly-created entities or a structured refusal.
    ///
    /// The default implementation is a no-op: only the HTTPS backend
    /// has anything to send.
    fn register_wrapper_assets(
        &self,
        _request: &ServiceWrapperRegisterRequest,
    ) -> Result<ServiceWrapperRegisterResponse> {
        Ok(ServiceWrapperRegisterResponse::default())
    }

    /// Return the runtime client instance id the backend authenticates
    /// this connection as. For the HTTPS service backend that is the
    /// mTLS-bound principal; in-memory test backends override with a
    /// fixed string.
    fn principal(&self) -> Result<String> {
        anyhow::bail!("backend does not report a principal");
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

/// Open the frontend backend client from daemon backend configuration.
/// The frontend only ever talks to a central backend over HTTPS.
pub fn open_backend_client(config: &BackendServiceConfig) -> Result<Box<dyn StorageBackend>> {
    Ok(Box::new(ServiceBackend::open(config)?))
}

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

    #[test]
    fn open_backend_client_rejects_missing_client_identity() {
        let err = match open_backend_client(&BackendServiceConfig {
            url: "https://backend.example.internal".to_string(),
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
        }) {
            Ok(_) => panic!("expected open to fail without client identity"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("client_cert_file"));
    }
}
