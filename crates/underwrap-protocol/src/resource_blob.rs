//! Canonical per-resource blob.
//!
//! A resource on the backend is one storage key whose value is a
//! JCS-canonical JSON document (RFC 8785) carrying the resource's
//! `env` values, `files` tree, and `libsecret` keyring in one
//! structure.
//!
//! Determinism rules — preserved across the entire schema, recursively:
//!
//! - Every map and set is a `BTreeMap` / `BTreeSet` so iteration
//!   order is key-sorted.
//! - No floating point numbers anywhere.
//! - No `serde(untagged)` enums.
//! - All `u64` fields are restricted to the ES6 safe-integer range
//!   (≤ 2⁵³ − 1). JCS encodes numbers per ECMAScript and will emit
//!   any value above that as a floating-point literal, which then
//!   fails to deserialize back into `u64`. Every `u64` in this schema
//!   is a unix-seconds timestamp, well below the limit for ~285M years
//!   past the epoch.
//!
//! Together with `serde_jcs::to_vec`, this guarantees that two blobs
//! with the same logical content serialize to byte-identical output.
//! Change detection at the wrapper end-of-run is a byte comparison
//! of `to_canonical_bytes(&before)` against `to_canonical_bytes(&after)`.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::message::SecretBytes;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Full content of one resource at one point in time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceBlob {
    pub schema_version: u32,
    #[serde(default)]
    pub env: BTreeMap<String, SecretBytes>,
    #[serde(default)]
    pub files: BTreeMap<String, SecretBytes>,
    #[serde(default)]
    pub libsecret: LibsecretSection,
}

/// Libsecret keyring state for one resource — the in-blob form of
/// what secretsd serves over D-Bus inside the namespace.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibsecretSection {
    /// Alias name → collection id (e.g. `"default" → "default"`).
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    /// Collection id → collection state.
    #[serde(default)]
    pub collections: BTreeMap<String, Collection>,
}

impl LibsecretSection {
    /// Serialize this section to JCS-canonical JSON bytes.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>> {
        serde_jcs::to_vec(self).context("serializing libsecret section (JCS)")
    }

    /// Deserialize a section from JSON. Call
    /// [`Self::from_strict_publication_bytes`] at the publication boundary.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("parsing libsecret section JSON")
    }

    /// Parse one host-selectable publication and require exact canonical bytes
    /// plus the complete identifier, reference, path-segment, and timestamp
    /// invariants of the private publication channel.
    pub fn from_strict_publication_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > crate::libsecret_publication::MAX_BODY_BYTES {
            return Err(anyhow!("libsecret publication exceeds the 1 MiB cap"));
        }
        let section: Self =
            serde_json::from_slice(bytes).context("parsing libsecret publication JSON")?;
        let canonical = section.to_canonical_bytes()?;
        if canonical != bytes {
            return Err(anyhow!(
                "libsecret publication is not exact JCS canonical bytes"
            ));
        }
        section.validate_publication_semantics()?;
        Ok(section)
    }

    pub fn validate_publication_semantics(&self) -> Result<()> {
        const MAX_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;
        fn segment(kind: &str, value: &str) -> Result<()> {
            if value.is_empty()
                || value.len() > 128
                || !value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(anyhow!("invalid {kind} path segment"));
            }
            Ok(())
        }
        fn timestamp(value: u64) -> Result<()> {
            if value > MAX_SAFE_INTEGER {
                return Err(anyhow!("libsecret timestamp exceeds 2^53 - 1"));
            }
            Ok(())
        }

        for (collection_id, collection) in &self.collections {
            segment("collection id", collection_id)?;
            timestamp(collection.created)?;
            timestamp(collection.modified)?;
            for (item_id, item) in &collection.items {
                if item_id.len() != 32
                    || !item_id
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(anyhow!("invalid item id"));
                }
                timestamp(item.created)?;
                timestamp(item.modified)?;
            }
        }
        for (alias, target) in &self.aliases {
            segment("alias", alias)?;
            segment("alias target", target)?;
            if !self.collections.contains_key(target) {
                return Err(anyhow!("libsecret alias names a missing collection"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub label: String,
    pub created: u64,
    pub modified: u64,
    #[serde(default)]
    pub items: BTreeMap<String, Item>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub label: String,
    pub content_type: String,
    pub created: u64,
    pub modified: u64,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub secret: SecretBytes,
}

impl ResourceBlob {
    /// Empty resource — schema version stamped, all sections empty.
    pub fn empty() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            env: BTreeMap::new(),
            files: BTreeMap::new(),
            libsecret: LibsecretSection::default(),
        }
    }

    /// Serialize to JCS-canonical JSON bytes. Two `ResourceBlob`s
    /// that are `==` produce byte-identical output here.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>> {
        serde_jcs::to_vec(self).context("serializing resource blob (JCS)")
    }

    /// Deserialize from JCS-canonical JSON bytes. Rejects unknown
    /// `schema_version` with a clear error so older daemons can't
    /// silently mis-parse a future-format blob.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self> {
        let blob: ResourceBlob =
            serde_json::from_slice(bytes).context("parsing resource blob JSON")?;
        if blob.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(anyhow!(
                "unsupported resource blob schema_version {} (expected {})",
                blob.schema_version,
                CURRENT_SCHEMA_VERSION
            ));
        }
        Ok(blob)
    }
}

impl Default for ResourceBlob {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests;
