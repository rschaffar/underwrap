use anyhow::{Context, Result};

use super::SecretStore;

impl SecretStore {
    /// Deserialize from a JSON envelope string.
    pub fn from_json(data: &str) -> Result<Self> {
        serde_json::from_str(data).context("deserializing secret store envelope")
    }

    /// Serialize to a JSON envelope string.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("serializing secret store envelope")
    }

    /// Serialize to a pretty-printed JSON envelope string.
    pub fn to_json_pretty(&self) -> Result<String> {
        serde_json::to_string_pretty(self).context("serializing secret store envelope")
    }
}
