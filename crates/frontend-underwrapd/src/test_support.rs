use std::collections::HashMap;
use std::sync::Mutex;

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::policy::{POLICY_DOCUMENT_SECRET_ID, PolicyDocument};
use underwrap_protocol::resource::{parse_secret_id, resource_blob_key};
use underwrap_store::{StorageBackend, blob_from_resource_entries};

pub(crate) struct MemStore {
    data: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemStore {
    /// Build a `MemStore` from per-slot entries, with each resource
    /// folded into one `resources/<id>` blob so the storage layout
    /// matches production. Non-slotted keys (e.g.
    /// `__underwrap/policy/document`) are stored verbatim.
    pub fn with_entries(entries: &[(&str, &[u8])]) -> Self {
        let mut by_resource: HashMap<String, HashMap<String, SecretBytes>> = HashMap::new();
        let mut raw: HashMap<String, Vec<u8>> = HashMap::new();
        for (key, value) in entries {
            if let Some(parsed) = parse_secret_id(key) {
                by_resource
                    .entry(parsed.resource_id.to_string())
                    .or_default()
                    .insert((*key).to_string(), SecretBytes::from((*value).to_vec()));
            } else {
                raw.insert((*key).to_string(), value.to_vec());
            }
        }
        for (resource_id, resource_entries) in &by_resource {
            let blob = blob_from_resource_entries(resource_id, resource_entries).unwrap();
            raw.insert(
                resource_blob_key(resource_id),
                blob.to_canonical_bytes().unwrap(),
            );
        }
        Self {
            data: Mutex::new(raw),
        }
    }

    pub fn with_entries_and_policy(entries: &[(&str, &[u8])], document: PolicyDocument) -> Self {
        let store = Self::with_entries(entries);
        store.data.lock().unwrap().insert(
            POLICY_DOCUMENT_SECRET_ID.to_string(),
            serde_json::to_vec(&document).unwrap(),
        );
        store
    }
}

impl StorageBackend for MemStore {
    fn read(&self, secret_id: &str) -> anyhow::Result<Option<SecretBytes>> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .get(secret_id)
            .cloned()
            .map(Into::into))
    }

    fn write(&self, secret_id: &str, material: &[u8]) -> anyhow::Result<()> {
        self.data
            .lock()
            .unwrap()
            .insert(secret_id.to_string(), material.to_vec());
        Ok(())
    }

    fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        let data = self.data.lock().unwrap();
        let mut ids: Vec<_> = data
            .keys()
            .filter(|id| id.starts_with(prefix))
            .cloned()
            .collect();
        ids.sort();
        Ok(ids)
    }

    fn delete(&self, secret_id: &str) -> anyhow::Result<bool> {
        Ok(self.data.lock().unwrap().remove(secret_id).is_some())
    }
}
