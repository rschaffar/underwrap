//! D-Bus Secret Service interfaces (org.freedesktop.Secret.Service,
//! .Collection, .Item, .Session).
//!
//! Only "plain" sessions are supported — all communication is over a
//! local Unix socket inside a PID namespace, so DH encryption is unnecessary.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use zbus::{
    fdo, interface,
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value},
};
use zeroize::Zeroize;

use underwrap_secretsd_store::SecretStore;

use crate::BASE_PATH;
use crate::channel::{self, ChannelOps};

/// Secret type for the D-Bus protocol.
#[derive(Debug, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct Secret {
    pub session: OwnedObjectPath,
    pub parameters: Vec<u8>,
    pub value: Vec<u8>,
    pub content_type: String,
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

// ---- Session ----

pub struct SessionInterface;

#[interface(name = "org.freedesktop.Secret.Session")]
impl SessionInterface {
    async fn close(&self) -> fdo::Result<()> {
        Ok(())
    }
}

// ---- Service ----

pub struct SecretService {
    store: Arc<RwLock<SecretStore>>,
    channel: Arc<dyn ChannelOps>,
}

impl SecretService {
    pub fn new(store: Arc<RwLock<SecretStore>>, channel: Arc<dyn ChannelOps>) -> Self {
        Self { store, channel }
    }
}

#[interface(name = "org.freedesktop.Secret.Service")]
impl SecretService {
    async fn open_session(
        &self,
        algorithm: &str,
        _input: OwnedValue,
        #[zbus(object_server)] object_server: &zbus::ObjectServer,
    ) -> fdo::Result<(Value<'_>, ObjectPath<'_>)> {
        if algorithm != "plain" {
            return Err(fdo::Error::NotSupported(
                "only 'plain' algorithm is supported".into(),
            ));
        }

        let session_path = format!("{BASE_PATH}/session/s0");
        let path = ObjectPath::try_from(session_path.clone())
            .map_err(|e| fdo::Error::Failed(format!("invalid path: {e}")))?;

        // Register session if not already there.
        let _ = object_server.at(&path, SessionInterface).await;

        Ok(("".into(), path))
    }

    async fn create_collection(
        &self,
        properties: HashMap<String, OwnedValue>,
        alias: &str,
        #[zbus(object_server)] object_server: &zbus::ObjectServer,
    ) -> fdo::Result<(ObjectPath<'_>, ObjectPath<'_>)> {
        let label: Option<String> = properties
            .get("org.freedesktop.Secret.Collection.Label")
            .and_then(|v| v.downcast_ref().ok());

        let alias = if alias.is_empty() { None } else { Some(alias) };

        let col_id = {
            let mut store = self.store.write().await;
            let id = store.create_collection(label.as_deref(), alias);
            channel::save_aliases(&*self.channel, &store)
                .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;
            channel::save_collection_meta(&*self.channel, &id, &store)
                .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;
            id
        };

        let path_str = format!("{BASE_PATH}/collection/{col_id}");
        let path = ObjectPath::try_from(path_str)
            .map_err(|e| fdo::Error::Failed(format!("invalid path: {e}")))?;

        let col = CollectionInterface::new(col_id, self.store.clone(), self.channel.clone());
        let _ = object_server.at(&path, col).await;

        let empty = ObjectPath::try_from("/").unwrap();
        Ok((path, empty))
    }

    async fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> fdo::Result<(Vec<ObjectPath<'_>>, Vec<ObjectPath<'_>>)> {
        let store = self.store.read().await;
        let results = store.search_all(&attributes);
        let mut paths = Vec::new();
        for (col_id, item_ids) in results {
            for item_id in item_ids {
                let path_str = format!("{BASE_PATH}/collection/{col_id}/{item_id}");
                if let Ok(p) = ObjectPath::try_from(path_str) {
                    paths.push(p);
                }
            }
        }
        // No locked items — we don't support locking.
        Ok((paths, vec![]))
    }

    async fn unlock(
        &self,
        objects: Vec<OwnedObjectPath>,
    ) -> fdo::Result<(Vec<OwnedObjectPath>, ObjectPath<'_>)> {
        // We don't support locking — say everything is unlocked.
        let empty = ObjectPath::try_from("/").unwrap();
        Ok((objects, empty))
    }

    async fn lock(
        &self,
        _objects: Vec<OwnedObjectPath>,
    ) -> fdo::Result<(Vec<ObjectPath<'_>>, ObjectPath<'_>)> {
        let empty = ObjectPath::try_from("/").unwrap();
        Ok((vec![], empty))
    }

    async fn get_secrets(
        &self,
        items: Vec<ObjectPath<'_>>,
        session: ObjectPath<'_>,
    ) -> fdo::Result<HashMap<OwnedObjectPath, Secret>> {
        let store = self.store.read().await;
        let mut results = HashMap::new();

        for item_path in items {
            if let Some((col_id, item_id)) = parse_item_path(item_path.as_str()) {
                // Read secret directly from backend — never from memory.
                if let Ok(Some(secret)) = channel::read_item_secret(&*self.channel, col_id, item_id)
                {
                    let content_type = store
                        .get_content_type(col_id, item_id)
                        .unwrap_or("text/plain");
                    results.insert(
                        item_path.into(),
                        Secret {
                            session: session.to_owned().into(),
                            parameters: vec![],
                            value: secret,
                            content_type: content_type.to_string(),
                        },
                    );
                }
            }
        }

        Ok(results)
    }

    async fn read_alias(&self, name: &str) -> fdo::Result<ObjectPath<'_>> {
        let store = self.store.read().await;
        if let Some(col_id) = store.get_alias(name) {
            let path_str = format!("{BASE_PATH}/collection/{col_id}");
            ObjectPath::try_from(path_str)
                .map_err(|e| fdo::Error::Failed(format!("invalid path: {e}")))
        } else {
            Ok(ObjectPath::try_from("/").unwrap())
        }
    }

    async fn set_alias(&self, name: &str, collection: ObjectPath<'_>) -> fdo::Result<()> {
        let col_id = if collection.as_str() == "/" {
            None
        } else {
            collection
                .as_str()
                .strip_prefix(&format!("{BASE_PATH}/collection/"))
                .map(|s| s.to_string())
        };

        let mut store = self.store.write().await;
        store.set_alias(name, col_id.as_deref());
        channel::save_aliases(&*self.channel, &store)
            .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;
        Ok(())
    }

    #[zbus(property)]
    async fn collections(&self) -> Vec<ObjectPath<'_>> {
        let store = self.store.read().await;
        store
            .list_collections()
            .into_iter()
            .filter_map(|id| ObjectPath::try_from(format!("{BASE_PATH}/collection/{id}")).ok())
            .collect()
    }
}

// ---- Collection ----

pub struct CollectionInterface {
    id: String,
    store: Arc<RwLock<SecretStore>>,
    channel: Arc<dyn ChannelOps>,
}

impl CollectionInterface {
    pub fn new(id: String, store: Arc<RwLock<SecretStore>>, channel: Arc<dyn ChannelOps>) -> Self {
        Self { id, store, channel }
    }
}

#[interface(name = "org.freedesktop.Secret.Collection")]
impl CollectionInterface {
    async fn delete(&self) -> fdo::Result<ObjectPath<'_>> {
        let mut store = self.store.write().await;

        // Delete items from backend (both .meta and .secret for each).
        let items = store.list_items(&self.id);
        for item_id in &items {
            let meta_key = format!("{}/{item_id}.meta", self.id);
            let secret_key = format!("{}/{item_id}.secret", self.id);
            let meta_deleted = self
                .channel
                .delete(&meta_key)
                .map_err(|e| fdo::Error::Failed(format!("delete error for {meta_key}: {e}")))?;
            let secret_deleted = self
                .channel
                .delete(&secret_key)
                .map_err(|e| fdo::Error::Failed(format!("delete error for {secret_key}: {e}")))?;
            if !meta_deleted || !secret_deleted {
                return Err(fdo::Error::Failed(format!(
                    "partial delete for item '{item_id}' in collection '{}': meta_deleted={meta_deleted}, secret_deleted={secret_deleted}",
                    self.id,
                )));
            }
        }
        let collection_meta_key = format!("{}/_meta", self.id);
        let collection_meta_deleted = self.channel.delete(&collection_meta_key).map_err(|e| {
            fdo::Error::Failed(format!("delete error for {collection_meta_key}: {e}"))
        })?;
        if !collection_meta_deleted {
            return Err(fdo::Error::Failed(format!(
                "collection metadata delete reported not found for '{}'",
                self.id,
            )));
        }

        store.delete_collection(&self.id);
        channel::save_aliases(&*self.channel, &store)
            .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;

        Ok(ObjectPath::try_from("/").unwrap())
    }

    async fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> fdo::Result<Vec<ObjectPath<'_>>> {
        let store = self.store.read().await;
        let matches = store.search(&self.id, &attributes);
        Ok(matches
            .into_iter()
            .filter_map(|item_id| {
                ObjectPath::try_from(format!("{BASE_PATH}/collection/{}/{item_id}", self.id)).ok()
            })
            .collect())
    }

    async fn create_item(
        &self,
        properties: HashMap<String, Value<'_>>,
        secret: Secret,
        replace: bool,
        #[zbus(object_server)] object_server: &zbus::ObjectServer,
    ) -> fdo::Result<(ObjectPath<'_>, ObjectPath<'_>)> {
        let label: String = properties
            .get("org.freedesktop.Secret.Item.Label")
            .and_then(|l| l.downcast_ref::<String>().ok())
            .unwrap_or_default();

        let attrs: HashMap<String, String> = properties
            .get("org.freedesktop.Secret.Item.Attributes")
            .and_then(|a| {
                // Try to extract HashMap<String, String> from the Dict value.
                let dict = a.downcast_ref::<zbus::zvariant::Dict>().ok()?;
                HashMap::<String, String>::try_from(dict).ok()
            })
            .unwrap_or_default();

        let (item_id, replaced) = {
            let mut store = self.store.write().await;
            // Store metadata only — secret bytes go directly to backend, not memory.
            let (id, replaced) = store
                .create_item(
                    &self.id,
                    &label,
                    &attrs,
                    &[], // Empty: secret not stored in memory.
                    &secret.content_type,
                    replace,
                )
                .map_err(|e| fdo::Error::Failed(format!("create_item error: {e}")))?;

            // Save item with actual secret to backend.
            channel::save_item_with_secret(&*self.channel, &self.id, &id, &store, &secret.value)
                .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;
            channel::save_collection_meta(&*self.channel, &self.id, &store)
                .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;
            (id, replaced)
        };

        let item_path_str = format!("{BASE_PATH}/collection/{}/{item_id}", self.id);
        let item_path = ObjectPath::try_from(item_path_str)
            .map_err(|e| fdo::Error::Failed(format!("invalid path: {e}")))?;

        if !replaced {
            let item = ItemInterface::new(
                item_id,
                self.id.clone(),
                self.store.clone(),
                self.channel.clone(),
            );
            let _ = object_server.at(&item_path, item).await;
        }

        let empty = ObjectPath::try_from("/").unwrap();
        Ok((item_path, empty))
    }

    #[zbus(property)]
    async fn items(&self) -> fdo::Result<Vec<ObjectPath<'_>>> {
        let store = self.store.read().await;
        Ok(store
            .list_items(&self.id)
            .into_iter()
            .filter_map(|item_id| {
                ObjectPath::try_from(format!("{BASE_PATH}/collection/{}/{item_id}", self.id)).ok()
            })
            .collect())
    }

    #[zbus(property)]
    async fn label(&self) -> fdo::Result<String> {
        let store = self.store.read().await;
        Ok(store
            .get_collection_label(&self.id)
            .unwrap_or("")
            .to_string())
    }

    #[zbus(property)]
    async fn set_label(&mut self, label: String) -> fdo::Result<()> {
        let mut store = self.store.write().await;
        store.set_collection_label(&self.id, &label);
        channel::save_collection_meta(&*self.channel, &self.id, &store)
            .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;
        Ok(())
    }

    #[zbus(property)]
    async fn locked(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn created(&self) -> fdo::Result<u64> {
        let store = self.store.read().await;
        Ok(store.get_collection_created(&self.id).unwrap_or(0))
    }

    #[zbus(property)]
    async fn modified(&self) -> fdo::Result<u64> {
        let store = self.store.read().await;
        Ok(store.get_collection_modified(&self.id).unwrap_or(0))
    }
}

// ---- Item ----

pub struct ItemInterface {
    id: String,
    collection_id: String,
    store: Arc<RwLock<SecretStore>>,
    channel: Arc<dyn ChannelOps>,
}

impl ItemInterface {
    pub fn new(
        id: String,
        collection_id: String,
        store: Arc<RwLock<SecretStore>>,
        channel: Arc<dyn ChannelOps>,
    ) -> Self {
        Self {
            id,
            collection_id,
            store,
            channel,
        }
    }
}

#[interface(name = "org.freedesktop.Secret.Item")]
impl ItemInterface {
    async fn delete(&self) -> fdo::Result<ObjectPath<'_>> {
        let mut store = self.store.write().await;

        // Delete from backend (both .meta and .secret).
        let meta_key = format!("{}/{}.meta", self.collection_id, self.id);
        let secret_key = format!("{}/{}.secret", self.collection_id, self.id);
        let meta_deleted = self
            .channel
            .delete(&meta_key)
            .map_err(|e| fdo::Error::Failed(format!("delete error for {meta_key}: {e}")))?;
        let secret_deleted = self
            .channel
            .delete(&secret_key)
            .map_err(|e| fdo::Error::Failed(format!("delete error for {secret_key}: {e}")))?;
        if !meta_deleted || !secret_deleted {
            return Err(fdo::Error::Failed(format!(
                "partial delete for item '{}' in collection '{}': meta_deleted={meta_deleted}, secret_deleted={secret_deleted}",
                self.id, self.collection_id,
            )));
        }

        store.delete_item(&self.collection_id, &self.id);
        // Item already deleted from backend above. Update collection modified time.
        channel::save_collection_meta(&*self.channel, &self.collection_id, &store)
            .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;

        Ok(ObjectPath::try_from("/").unwrap())
    }

    async fn get_secret(&self, session: ObjectPath<'_>) -> fdo::Result<Secret> {
        // Read secret directly from backend — never from memory.
        let secret = channel::read_item_secret(&*self.channel, &self.collection_id, &self.id)
            .map_err(|e| fdo::Error::Failed(format!("read error: {e}")))?;
        let secret = secret.ok_or_else(|| fdo::Error::Failed("item not found".into()))?;

        let store = self.store.read().await;
        let content_type = store
            .get_content_type(&self.collection_id, &self.id)
            .unwrap_or("text/plain");

        Ok(Secret {
            session: session.to_owned().into(),
            parameters: vec![],
            value: secret,
            content_type: content_type.to_string(),
        })
    }

    async fn set_secret(&self, secret: Secret) -> fdo::Result<()> {
        let store = self.store.read().await;
        // Write secret directly to backend — not stored in memory.
        channel::save_item_with_secret(
            &*self.channel,
            &self.collection_id,
            &self.id,
            &store,
            &secret.value,
        )
        .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;
        Ok(())
    }

    #[zbus(property)]
    async fn locked(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn attributes(&self) -> fdo::Result<HashMap<String, String>> {
        let store = self.store.read().await;
        Ok(store
            .get_attributes(&self.collection_id, &self.id)
            .cloned()
            .unwrap_or_default())
    }

    #[zbus(property)]
    async fn set_attributes(&mut self, attributes: HashMap<String, String>) -> fdo::Result<()> {
        let mut store = self.store.write().await;
        store.set_attributes(&self.collection_id, &self.id, attributes);
        channel::save_item_metadata(&*self.channel, &self.collection_id, &self.id, &store)
            .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;
        Ok(())
    }

    #[zbus(property)]
    async fn label(&self) -> fdo::Result<String> {
        let store = self.store.read().await;
        Ok(store
            .get_label(&self.collection_id, &self.id)
            .unwrap_or("")
            .to_string())
    }

    #[zbus(property)]
    async fn set_label(&mut self, label: String) -> fdo::Result<()> {
        let mut store = self.store.write().await;
        store.set_label(&self.collection_id, &self.id, &label);
        channel::save_item_metadata(&*self.channel, &self.collection_id, &self.id, &store)
            .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;
        Ok(())
    }

    #[zbus(property)]
    async fn created(&self) -> fdo::Result<u64> {
        let store = self.store.read().await;
        Ok(store
            .get_created(&self.collection_id, &self.id)
            .unwrap_or(0))
    }

    #[zbus(property)]
    async fn modified(&self) -> fdo::Result<u64> {
        let store = self.store.read().await;
        Ok(store
            .get_modified(&self.collection_id, &self.id)
            .unwrap_or(0))
    }
}

// ---- Helpers ----

/// Parse an item object path like "/org/freedesktop/secrets/collection/default/item_1"
/// into (collection_id, item_id).
fn parse_item_path(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix(&format!("{BASE_PATH}/collection/"))?;
    let (col, item) = rest.split_once('/')?;
    if col.is_empty() || item.is_empty() {
        None
    } else {
        Some((col, item))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{ChannelOps, ItemMeta};
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex as StdMutex;

    struct FakeChannel {
        data: StdMutex<HashMap<String, Vec<u8>>>,
        fail_delete: HashSet<String>,
    }

    impl FakeChannel {
        fn new(data: HashMap<String, Vec<u8>>) -> Self {
            Self {
                data: StdMutex::new(data),
                fail_delete: HashSet::new(),
            }
        }

        fn with_failing_delete(mut self, key: &str) -> Self {
            self.fail_delete.insert(key.to_string());
            self
        }
    }

    impl ChannelOps for FakeChannel {
        fn read(
            &self,
            secret_id: &str,
        ) -> anyhow::Result<Option<underwrap_protocol::message::SecretBytes>> {
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

        fn delete(&self, secret_id: &str) -> anyhow::Result<bool> {
            if self.fail_delete.contains(secret_id) {
                anyhow::bail!("forced delete failure for {secret_id}");
            }
            Ok(self.data.lock().unwrap().remove(secret_id).is_some())
        }
    }

    fn item_meta_json() -> Vec<u8> {
        serde_json::to_vec(&ItemMeta {
            content_type: "text/plain".to_string(),
            attributes: HashMap::new(),
            label: "Item".to_string(),
            created: 1,
            modified: 1,
        })
        .unwrap()
    }

    #[tokio::test]
    async fn item_delete_propagates_backend_error_and_keeps_state() {
        let mut store = SecretStore::new_default();
        let item_id = store
            .create_item(
                "default",
                "Item",
                &HashMap::new(),
                b"secret",
                "text/plain",
                false,
            )
            .unwrap()
            .0;
        let channel: Arc<dyn ChannelOps> = Arc::new(
            FakeChannel::new(HashMap::from([
                (format!("default/{item_id}.meta"), item_meta_json()),
                (format!("default/{item_id}.secret"), b"secret".to_vec()),
            ]))
            .with_failing_delete(&format!("default/{item_id}.secret")),
        );
        let store = Arc::new(RwLock::new(store));
        let item = ItemInterface::new(
            item_id.clone(),
            "default".to_string(),
            store.clone(),
            channel,
        );

        let err = item.delete().await.unwrap_err();
        assert!(matches!(err, fdo::Error::Failed(_)));
        assert!(
            store
                .read()
                .await
                .list_items("default")
                .contains(&item_id.as_str())
        );
    }

    #[tokio::test]
    async fn collection_delete_propagates_backend_error_and_keeps_state() {
        let mut store = SecretStore::new_default();
        let col_id = store.create_collection(Some("Test"), None);
        let item_id = store
            .create_item(
                &col_id,
                "Item",
                &HashMap::new(),
                b"secret",
                "text/plain",
                false,
            )
            .unwrap()
            .0;
        let channel: Arc<dyn ChannelOps> = Arc::new(
            FakeChannel::new(HashMap::from([
                (format!("{col_id}/{item_id}.meta"), item_meta_json()),
                (format!("{col_id}/{item_id}.secret"), b"secret".to_vec()),
                (format!("{col_id}/_meta"), b"{}".to_vec()),
            ]))
            .with_failing_delete(&format!("{col_id}/{item_id}.secret")),
        );
        let store = Arc::new(RwLock::new(store));
        let col = CollectionInterface::new(col_id.clone(), store.clone(), channel);

        let err = col.delete().await.unwrap_err();
        assert!(matches!(err, fdo::Error::Failed(_)));
        assert!(store.read().await.collections.contains_key(&col_id));
    }
}
