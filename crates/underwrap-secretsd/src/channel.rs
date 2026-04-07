//! Daemon channel — synchronous IPC over the authenticated socketpair.
//!
//! Provides read/write/list/delete operations that map to the daemon's
//! StorageBackend via the secretsd channel protocol.

use anyhow::{Context, Result, bail};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;

use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{SecretsdRequest, SecretsdResponse};
use underwrap_secretsd_store::SecretStore;

/// A synchronous channel to the daemon over the socketpair.
pub struct DaemonChannel {
    stream: Mutex<UnixStream>,
}

impl DaemonChannel {
    pub fn new(stream: UnixStream) -> Self {
        Self {
            stream: Mutex::new(stream),
        }
    }

    pub fn read(&self, secret_id: &str) -> Result<Option<String>> {
        let req = SecretsdRequest::Read {
            secret_id: secret_id.to_string(),
        };
        let resp = self.send(&req)?;
        match resp {
            SecretsdResponse::Ok { material, .. } => Ok(material),
            SecretsdResponse::Error { message } => bail!("daemon read error: {message}"),
        }
    }

    pub fn write(&self, secret_id: &str, material: &str) -> Result<()> {
        let req = SecretsdRequest::Write {
            secret_id: secret_id.to_string(),
            material: material.to_string(),
        };
        let resp = self.send(&req)?;
        match resp {
            SecretsdResponse::Ok { .. } => Ok(()),
            SecretsdResponse::Error { message } => bail!("daemon write error: {message}"),
        }
    }

    pub fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let req = SecretsdRequest::List {
            prefix: prefix.to_string(),
        };
        let resp = self.send(&req)?;
        match resp {
            SecretsdResponse::Ok { ids, .. } => Ok(ids.unwrap_or_default()),
            SecretsdResponse::Error { message } => bail!("daemon list error: {message}"),
        }
    }

    pub fn delete(&self, secret_id: &str) -> Result<bool> {
        let req = SecretsdRequest::Delete {
            secret_id: secret_id.to_string(),
        };
        let resp = self.send(&req)?;
        match resp {
            SecretsdResponse::Ok { existed, .. } => Ok(existed.unwrap_or(false)),
            SecretsdResponse::Error { message } => bail!("daemon delete error: {message}"),
        }
    }

    fn send(&self, req: &SecretsdRequest) -> Result<SecretsdResponse> {
        let mut stream = self.stream.lock().unwrap();
        write_frame_sync(&mut *stream, req).context("sending to daemon channel")?;
        read_frame_sync(&mut *stream).context("reading from daemon channel")
    }
}

/// Load the initial Secret Service state from the daemon by reading
/// existing items from the backend.
pub fn load_initial_state(channel: &DaemonChannel) -> Result<SecretStore> {
    // Try to read the aliases file.
    let aliases_json = channel.read("_aliases")?;
    let mut store = SecretStore::new_default();

    if let Some(aliases_str) = aliases_json {
        // Parse aliases.
        let aliases: std::collections::HashMap<String, String> =
            serde_json::from_str(&aliases_str).context("parsing _aliases")?;

        for (alias, col_id) in &aliases {
            // Load collection metadata.
            let meta_key = format!("{col_id}/_meta");
            if let Some(meta_str) = channel.read(&meta_key)? {
                let meta: CollectionMeta =
                    serde_json::from_str(&meta_str).context("parsing collection meta")?;

                if !store.collections.contains_key(col_id) {
                    store.collections.insert(
                        col_id.clone(),
                        underwrap_secretsd_store::Collection {
                            label: meta.label,
                            items: std::collections::HashMap::new(),
                            created: meta.created,
                            modified: meta.modified,
                        },
                    );
                }
            }
            store.aliases.insert(alias.clone(), col_id.clone());
        }
    }

    // For each collection, load its items.
    let col_ids: Vec<String> = store.collections.keys().cloned().collect();
    for col_id in &col_ids {
        let items = channel.list(col_id)?;
        for item_key in items {
            // Skip _meta files.
            if item_key.ends_with("/_meta") || item_key == "_aliases" {
                continue;
            }
            // item_key is like "default/item_1"
            let item_id = item_key
                .strip_prefix(&format!("{col_id}/"))
                .unwrap_or(&item_key);

            if let Some(item_str) = channel.read(&item_key)? {
                let item: ItemData =
                    serde_json::from_str(&item_str).context("parsing item data")?;

                if let Some(col) = store.collections.get_mut(col_id) {
                    col.items.insert(
                        item_id.to_string(),
                        underwrap_secretsd_store::Item {
                            secret: item.secret,
                            content_type: item.content_type,
                            attributes: item.attributes,
                            label: item.label,
                            created: item.created,
                            modified: item.modified,
                        },
                    );
                }
            }
        }
    }

    Ok(store)
}

/// Persist the entire store state back to the daemon.
/// Called after any mutation.
pub fn save_state(channel: &DaemonChannel, store: &SecretStore) -> Result<()> {
    // Save aliases.
    let aliases_json = serde_json::to_string(&store.aliases)?;
    channel.write("_aliases", &aliases_json)?;

    // Save each collection's metadata and items.
    for (col_id, col) in &store.collections {
        let meta = CollectionMeta {
            label: col.label.clone(),
            created: col.created,
            modified: col.modified,
        };
        let meta_json = serde_json::to_string(&meta)?;
        channel.write(&format!("{col_id}/_meta"), &meta_json)?;

        for (item_id, item) in &col.items {
            let item_data = ItemData {
                secret: item.secret.clone(),
                content_type: item.content_type.clone(),
                attributes: item.attributes.clone(),
                label: item.label.clone(),
                created: item.created,
                modified: item.modified,
            };
            let item_json = serde_json::to_string(&item_data)?;
            channel.write(&format!("{col_id}/{item_id}"), &item_json)?;
        }
    }

    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CollectionMeta {
    label: String,
    created: u64,
    modified: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ItemData {
    #[serde(with = "base64_bytes")]
    secret: Vec<u8>,
    #[serde(default = "default_content_type")]
    content_type: String,
    attributes: std::collections::HashMap<String, String>,
    label: String,
    created: u64,
    modified: u64,
}

fn default_content_type() -> String {
    "text/plain".to_string()
}

mod base64_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        encoded.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        use base64::Engine;
        let encoded = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .map_err(serde::de::Error::custom)
    }
}
