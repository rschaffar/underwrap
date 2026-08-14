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

/// Identifier-only reference to one stored entry in a resource blob.
///
/// This is the complete lifecycle view used by resource deletion blockers.
/// It deliberately carries only map keys: no values, labels, alias targets,
/// attributes, content types, timestamps, or secret bytes are copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceContentReference {
    Env {
        name: String,
    },
    File {
        path: String,
    },
    LibsecretAlias {
        name: String,
    },
    LibsecretCollection {
        collection_id: String,
    },
    LibsecretItem {
        collection_id: String,
        item_id: String,
    },
}

impl utoipa::PartialSchema for ResourceContentReference {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        use utoipa::openapi::schema::{Discriminator, Object, OneOf, Schema, Type};

        fn tagged_object(kind: &'static str, fields: &[&'static str]) -> Schema {
            let mut object = Object::builder()
                .property(
                    "kind",
                    Object::builder()
                        .schema_type(Type::String)
                        .enum_values(Some([kind])),
                )
                .required("kind");
            for field in fields {
                object = object
                    .property(*field, Object::with_type(Type::String))
                    .required(*field);
            }
            object.build().into()
        }

        OneOf::builder()
            .item(tagged_object("env", &["name"]))
            .item(tagged_object("file", &["path"]))
            .item(tagged_object("libsecret_alias", &["name"]))
            .item(tagged_object("libsecret_collection", &["collection_id"]))
            .item(tagged_object(
                "libsecret_item",
                &["collection_id", "item_id"],
            ))
            .discriminator(Some(Discriminator::new("kind")))
            .into()
    }
}

impl utoipa::ToSchema for ResourceContentReference {}

/// Inclusive maximum for the complete JCS-canonical resource blob.
///
/// This is deliberately independent of the private libsecret publication
/// body limit and all transport envelopes.
pub const MAX_CANONICAL_RESOURCE_BYTES: usize = 2 * 1024 * 1024;

/// A complete canonical resource exceeded the public size authority.
///
/// This preserves the stable diagnostic text while allowing callers that need
/// safe size metadata to classify the failure without inspecting a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceTooLargeError {
    attempted_len: usize,
    maximum_len: usize,
}

impl ResourceTooLargeError {
    const fn new(attempted_len: usize) -> Self {
        Self {
            attempted_len,
            maximum_len: MAX_CANONICAL_RESOURCE_BYTES,
        }
    }

    pub const fn attempted_len(self) -> usize {
        self.attempted_len
    }

    pub const fn maximum_len(self) -> usize {
        self.maximum_len
    }
}

impl std::fmt::Display for ResourceTooLargeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "resource blob exceeds maximum of {} bytes (got {})",
            self.maximum_len, self.attempted_len
        )
    }
}

impl std::error::Error for ResourceTooLargeError {}

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

    /// Enumerate every stored content entry in deterministic lifecycle order.
    ///
    /// Env keys, file keys, and aliases come first. Collections follow, each
    /// immediately followed by its items. Every map uses its `BTreeMap` order,
    /// and even a collection with no items contributes one reference.
    pub fn content_references(&self) -> Vec<ResourceContentReference> {
        let mut references = Vec::with_capacity(
            self.env
                .len()
                .saturating_add(self.files.len())
                .saturating_add(self.libsecret.aliases.len())
                .saturating_add(self.libsecret.collections.len())
                .saturating_add(
                    self.libsecret
                        .collections
                        .values()
                        .fold(0usize, |count, collection| {
                            count.saturating_add(collection.items.len())
                        }),
                ),
        );
        references.extend(
            self.env
                .keys()
                .map(|name| ResourceContentReference::Env { name: name.clone() }),
        );
        references.extend(
            self.files
                .keys()
                .map(|path| ResourceContentReference::File { path: path.clone() }),
        );
        references.extend(
            self.libsecret
                .aliases
                .keys()
                .map(|name| ResourceContentReference::LibsecretAlias { name: name.clone() }),
        );
        for (collection_id, collection) in &self.libsecret.collections {
            references.push(ResourceContentReference::LibsecretCollection {
                collection_id: collection_id.clone(),
            });
            references.extend(collection.items.keys().map(|item_id| {
                ResourceContentReference::LibsecretItem {
                    collection_id: collection_id.clone(),
                    item_id: item_id.clone(),
                }
            }));
        }
        references
    }

    /// Serialize to JCS-canonical JSON bytes. Two `ResourceBlob`s
    /// that are `==` produce byte-identical output here.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>> {
        let bytes = serde_jcs::to_vec(self).context("serializing resource blob (JCS)")?;
        ensure_resource_size(bytes.len())?;
        Ok(bytes)
    }

    /// Deserialize from JCS-canonical JSON bytes. Rejects unknown
    /// `schema_version` with a clear error so older daemons can't
    /// silently mis-parse a future-format blob.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self> {
        ensure_resource_size(bytes.len())?;
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

/// Reject a complete canonical resource that exceeds the protocol authority.
pub fn ensure_resource_size(len: usize) -> Result<()> {
    if len > MAX_CANONICAL_RESOURCE_BYTES {
        return Err(ResourceTooLargeError::new(len).into());
    }
    Ok(())
}

/// Return complete-resource size metadata when the error came from the shared
/// size authority.
pub fn resource_too_large_error(error: &anyhow::Error) -> Option<&ResourceTooLargeError> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<ResourceTooLargeError>())
}

/// Whether an error came from the complete-resource size authority.
pub fn is_resource_too_large_error(error: &anyhow::Error) -> bool {
    resource_too_large_error(error).is_some()
}

impl Default for ResourceBlob {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests;
