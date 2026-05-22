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
    /// Serialize this section to JCS-canonical JSON bytes. Used by
    /// the ns↔secretsd init/teardown pipes so the section can be
    /// exchanged independently of the surrounding `ResourceBlob`.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>> {
        serde_jcs::to_vec(self).context("serializing libsecret section (JCS)")
    }

    /// Deserialize a section from JCS-canonical JSON bytes.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).context("parsing libsecret section JSON")
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
