use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A complete Secret Service state: collections, items, aliases.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecretStore {
    /// Map of collection ID → collection.
    pub collections: HashMap<String, Collection>,
    /// Map of alias name → collection ID.
    pub aliases: HashMap<String, String>,
    /// Counter for generating unique IDs.
    #[serde(default)]
    pub(crate) next_id: u64,
}

/// A Secret Service collection containing items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Collection {
    pub label: String,
    pub items: HashMap<String, Item>,
    pub created: u64,
    pub modified: u64,
}

/// A Secret Service item: secret value + metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    /// The secret value (raw bytes, base64-encoded in JSON for safety).
    #[serde(with = "super::helpers::base64_bytes")]
    pub secret: Vec<u8>,
    /// MIME content type of the secret.
    #[serde(default = "super::helpers::default_content_type")]
    pub content_type: String,
    /// Search attributes (key-value pairs).
    pub attributes: HashMap<String, String>,
    /// Human-readable label.
    pub label: String,
    pub created: u64,
    pub modified: u64,
}
