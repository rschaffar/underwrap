//! Daemon channel — synchronous IPC over the authenticated socketpair.
//!
//! Provides read/write/list/delete operations that map to the daemon's
//! StorageBackend via the secretsd channel protocol.

use anyhow::{Context, Result, bail};
use std::os::unix::net::UnixStream;
use std::sync::Mutex;

use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{SecretsdErrorCode, SecretsdRequest, SecretsdResponse};
use underwrap_secretsd_store::SecretStore;

pub trait ChannelOps: Send + Sync {
    fn read(&self, secret_id: &str) -> Result<Option<String>>;
    fn write(&self, secret_id: &str, material: &str) -> Result<()>;
    fn list(&self, prefix: &str) -> Result<Vec<String>>;
    fn delete(&self, secret_id: &str) -> Result<bool>;
}

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

    fn send(&self, req: &SecretsdRequest) -> Result<SecretsdResponse> {
        let mut stream = self.stream.lock().unwrap();
        write_frame_sync(&mut *stream, req).context("sending to daemon channel")?;
        read_frame_sync(&mut *stream).context("reading from daemon channel")
    }
}

impl ChannelOps for DaemonChannel {
    fn read(&self, secret_id: &str) -> Result<Option<String>> {
        let req = SecretsdRequest::Read {
            secret_id: secret_id.to_string(),
        };
        let resp = self.send(&req)?;
        match resp {
            SecretsdResponse::Ok { material, .. } => Ok(material.map(Into::into)),
            SecretsdResponse::Error { code, message } => match code {
                SecretsdErrorCode::NotFound => Ok(None),
                _ => bail!("daemon read error ({code:?}): {message}"),
            },
        }
    }

    fn write(&self, secret_id: &str, material: &str) -> Result<()> {
        let req = SecretsdRequest::Write {
            secret_id: secret_id.to_string(),
            material: material.to_string().into(),
        };
        let resp = self.send(&req)?;
        match resp {
            SecretsdResponse::Ok { .. } => Ok(()),
            SecretsdResponse::Error { code, message } => {
                bail!("daemon write error ({code:?}): {message}")
            }
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let req = SecretsdRequest::List {
            prefix: prefix.to_string(),
        };
        let resp = self.send(&req)?;
        match resp {
            SecretsdResponse::Ok { ids, .. } => Ok(ids.unwrap_or_default()),
            SecretsdResponse::Error { code, message } => {
                bail!("daemon list error ({code:?}): {message}")
            }
        }
    }

    fn delete(&self, secret_id: &str) -> Result<bool> {
        let req = SecretsdRequest::Delete {
            secret_id: secret_id.to_string(),
        };
        let resp = self.send(&req)?;
        match resp {
            SecretsdResponse::Ok { existed, .. } => Ok(existed.unwrap_or(false)),
            SecretsdResponse::Error { code, message } => {
                bail!("daemon delete error ({code:?}): {message}")
            }
        }
    }
}

/// Load the initial Secret Service state from the daemon.
/// Secret values are NOT kept in memory — only metadata (attributes,
/// labels, timestamps) is loaded for D-Bus object registration and search.
pub fn load_initial_state(channel: &(impl ChannelOps + ?Sized)) -> Result<SecretStore> {
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

    // For each collection, load item metadata (NOT secret values).
    let col_ids: Vec<String> = store.collections.keys().cloned().collect();
    for col_id in &col_ids {
        let items = channel.list(col_id)?;
        let prefix = format!("{col_id}/");
        let mut meta_ids = std::collections::BTreeSet::new();
        let mut secret_ids = std::collections::BTreeSet::new();

        for item_key in &items {
            if item_key == "_aliases" || item_key.ends_with("/_meta") {
                continue;
            }
            if let Some(rest) = item_key.strip_prefix(&prefix) {
                if let Some(item_id) = rest.strip_suffix(".meta") {
                    meta_ids.insert(item_id.to_string());
                } else if let Some(item_id) = rest.strip_suffix(".secret") {
                    secret_ids.insert(item_id.to_string());
                }
            }
        }

        for orphan_secret in secret_ids.difference(&meta_ids) {
            let orphan_key = format!("{col_id}/{orphan_secret}.secret");
            let deleted = channel
                .delete(&orphan_key)
                .with_context(|| format!("scrubbing orphan secret '{orphan_key}'"))?;
            anyhow::ensure!(deleted, "orphan secret '{orphan_key}' could not be deleted");
        }

        let orphan_meta: Vec<String> = meta_ids.difference(&secret_ids).cloned().collect();
        anyhow::ensure!(
            orphan_meta.is_empty(),
            "collection '{col_id}' contains metadata without secrets for item(s): {}",
            orphan_meta.join(", ")
        );

        for item_id in &meta_ids {
            let item_key = format!("{col_id}/{item_id}.meta");
            if let Some(meta_str) = channel.read(&item_key)? {
                let meta: ItemMeta =
                    serde_json::from_str(&meta_str).context("parsing item metadata")?;

                if let Some(col) = store.collections.get_mut(col_id) {
                    col.items.insert(
                        item_id.to_string(),
                        underwrap_secretsd_store::Item {
                            secret: Vec::new(),
                            content_type: meta.content_type,
                            attributes: meta.attributes,
                            label: meta.label,
                            created: meta.created,
                            modified: meta.modified,
                        },
                    );
                }
            }
        }
    }

    Ok(store)
}

/// Read an item's secret value directly from the backend.
/// Secret bytes are never kept in secretsd's memory beyond the
/// scope of the D-Bus method call that requested them.
pub fn read_item_secret(
    channel: &(impl ChannelOps + ?Sized),
    col_id: &str,
    item_id: &str,
) -> Result<Option<Vec<u8>>> {
    let key = format!("{col_id}/{item_id}.secret");
    if let Some(secret_b64) = channel.read(&key)? {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&secret_b64)
            .context("decoding item secret base64")?;
        Ok(Some(bytes))
    } else {
        Ok(None)
    }
}

/// Save only the aliases to the daemon.
pub fn save_aliases(channel: &(impl ChannelOps + ?Sized), store: &SecretStore) -> Result<()> {
    let aliases_json = serde_json::to_string(&store.aliases)?;
    channel.write("_aliases", &aliases_json)
}

/// Save a single collection's metadata to the daemon.
pub fn save_collection_meta(
    channel: &(impl ChannelOps + ?Sized),
    col_id: &str,
    store: &SecretStore,
) -> Result<()> {
    if let Some(col) = store.collections.get(col_id) {
        let meta = CollectionMeta {
            label: col.label.clone(),
            created: col.created,
            modified: col.modified,
        };
        let meta_json = serde_json::to_string(&meta)?;
        channel.write(&format!("{col_id}/_meta"), &meta_json)?;
    }
    Ok(())
}

/// Save a single item: metadata to .meta key, secret to .secret key.
pub fn save_item_with_secret(
    channel: &(impl ChannelOps + ?Sized),
    col_id: &str,
    item_id: &str,
    store: &SecretStore,
    secret: &[u8],
) -> Result<()> {
    if let Some(item) = store
        .collections
        .get(col_id)
        .and_then(|c| c.items.get(item_id))
    {
        // Write metadata (no secret).
        let meta = ItemMeta {
            content_type: item.content_type.clone(),
            attributes: item.attributes.clone(),
            label: item.label.clone(),
            created: item.created,
            modified: item.modified,
        };
        let meta_json = serde_json::to_string(&meta)?;
        channel.write(&format!("{col_id}/{item_id}.meta"), &meta_json)?;

        // Write secret separately (base64-encoded).
        use base64::Engine;
        let secret_b64 = base64::engine::general_purpose::STANDARD.encode(secret);
        channel.write(&format!("{col_id}/{item_id}.secret"), &secret_b64)?;
    }
    Ok(())
}

/// Save item metadata only (does not touch the .secret key).
pub fn save_item_metadata(
    channel: &(impl ChannelOps + ?Sized),
    col_id: &str,
    item_id: &str,
    store: &SecretStore,
) -> Result<()> {
    if let Some(item) = store
        .collections
        .get(col_id)
        .and_then(|c| c.items.get(item_id))
    {
        let meta = ItemMeta {
            content_type: item.content_type.clone(),
            attributes: item.attributes.clone(),
            label: item.label.clone(),
            created: item.created,
            modified: item.modified,
        };
        let meta_json = serde_json::to_string(&meta)?;
        channel.write(&format!("{col_id}/{item_id}.meta"), &meta_json)?;
    }
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CollectionMeta {
    label: String,
    created: u64,
    modified: u64,
}

/// Item metadata (no secret). Used for .meta keys.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ItemMeta {
    #[serde(default = "default_content_type")]
    pub content_type: String,
    pub attributes: std::collections::HashMap<String, String>,
    pub label: String,
    pub created: u64,
    pub modified: u64,
}

fn default_content_type() -> String {
    "text/plain".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    struct FakeChannel {
        data: Mutex<HashMap<String, String>>,
        fail_delete: HashSet<String>,
        deleted: Mutex<Vec<String>>,
    }

    impl FakeChannel {
        fn new(data: HashMap<String, String>) -> Self {
            Self {
                data: Mutex::new(data),
                fail_delete: HashSet::new(),
                deleted: Mutex::new(Vec::new()),
            }
        }

        fn with_failing_delete(mut self, key: &str) -> Self {
            self.fail_delete.insert(key.to_string());
            self
        }
    }

    impl ChannelOps for FakeChannel {
        fn read(&self, secret_id: &str) -> Result<Option<String>> {
            Ok(self.data.lock().unwrap().get(secret_id).cloned())
        }

        fn write(&self, secret_id: &str, material: &str) -> Result<()> {
            self.data
                .lock()
                .unwrap()
                .insert(secret_id.to_string(), material.to_string());
            Ok(())
        }

        fn list(&self, prefix: &str) -> Result<Vec<String>> {
            let prefix = format!("{prefix}/");
            Ok(self
                .data
                .lock()
                .unwrap()
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .cloned()
                .collect())
        }

        fn delete(&self, secret_id: &str) -> Result<bool> {
            if self.fail_delete.contains(secret_id) {
                bail!("forced delete failure for {secret_id}");
            }
            self.deleted.lock().unwrap().push(secret_id.to_string());
            Ok(self.data.lock().unwrap().remove(secret_id).is_some())
        }
    }

    fn item_meta_json() -> String {
        serde_json::to_string(&ItemMeta {
            content_type: "text/plain".to_string(),
            attributes: HashMap::new(),
            label: "Item".to_string(),
            created: 1,
            modified: 1,
        })
        .unwrap()
    }

    #[test]
    fn load_initial_state_scrubs_orphan_secret() {
        let mut data = HashMap::new();
        data.insert("default/item_1.secret".to_string(), "c2VjcmV0".to_string());
        let channel = FakeChannel::new(data);

        let store = load_initial_state(&channel).unwrap();
        assert!(store.list_items("default").is_empty());
        assert_eq!(
            channel.deleted.lock().unwrap().as_slice(),
            &["default/item_1.secret".to_string()]
        );
    }

    #[test]
    fn load_initial_state_fails_on_orphan_meta() {
        let mut data = HashMap::new();
        data.insert("default/item_1.meta".to_string(), item_meta_json());
        let channel = FakeChannel::new(data);

        let err = load_initial_state(&channel).unwrap_err();
        assert!(err.to_string().contains("metadata without secrets"));
    }

    #[test]
    fn load_initial_state_fails_if_orphan_secret_scrub_fails() {
        let mut data = HashMap::new();
        data.insert("default/item_1.secret".to_string(), "c2VjcmV0".to_string());
        let channel = FakeChannel::new(data).with_failing_delete("default/item_1.secret");

        let err = load_initial_state(&channel).unwrap_err();
        assert!(err.to_string().contains("scrubbing orphan secret"));
    }
}
