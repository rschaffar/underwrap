use std::collections::HashMap;
use std::sync::Mutex;

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::policy::{POLICY_DOCUMENT_SECRET_ID, PolicyDocument};
use underwrap_store::StorageBackend;

pub(crate) struct MemStore {
    data: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemStore {
    pub fn with_entries(entries: &[(&str, &[u8])]) -> Self {
        Self {
            data: Mutex::new(
                entries
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), v.to_vec()))
                    .collect(),
            ),
        }
    }

    pub fn with_entries_and_policy(entries: &[(&str, &[u8])], document: PolicyDocument) -> Self {
        let mut data: HashMap<String, Vec<u8>> = entries
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.to_vec()))
            .collect();
        data.insert(
            POLICY_DOCUMENT_SECRET_ID.to_string(),
            serde_json::to_vec(&document).unwrap(),
        );
        Self {
            data: Mutex::new(data),
        }
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
