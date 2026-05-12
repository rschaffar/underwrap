use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use zbus::{
    fdo, interface,
    zvariant::{ObjectPath, Value},
};

use underwrap_secretsd_store::SecretStore;

use crate::channel::{self, ChannelOps};
use crate::metadata_trace_enabled;

use super::{ItemInterface, Secret, empty_path, item_path};

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

        Ok(empty_path())
    }

    async fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> fdo::Result<Vec<ObjectPath<'_>>> {
        let store = self.store.read().await;
        let matches = store.search(&self.id, &attributes);
        let paths: Vec<ObjectPath<'_>> = matches
            .into_iter()
            .filter_map(|item_id| item_path(&self.id, item_id))
            .collect();
        if metadata_trace_enabled() {
            tracing::info!(
                collection = %self.id,
                ?attributes,
                result_count = paths.len(),
                "secretsd Collection.SearchItems"
            );
        }
        Ok(paths)
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
                let dict = a.downcast_ref::<zbus::zvariant::Dict>().ok()?;
                HashMap::<String, String>::try_from(dict).ok()
            })
            .unwrap_or_default();

        let secret_len = secret.value.len();
        let content_type = secret.content_type.clone();
        let (item_id, replaced) = {
            let mut store = self.store.write().await;
            let (id, replaced) = store
                .create_item(&self.id, &label, &attrs, &[], &secret.content_type, replace)
                .map_err(|e| fdo::Error::Failed(format!("create_item error: {e}")))?;

            channel::save_item_with_secret(&*self.channel, &self.id, &id, &store, &secret.value)
                .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;
            channel::save_collection_meta(&*self.channel, &self.id, &store)
                .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;
            (id, replaced)
        };

        if metadata_trace_enabled() {
            tracing::info!(
                collection = %self.id,
                item = %item_id,
                label,
                ?attrs,
                replace,
                replaced,
                content_type,
                secret_len,
                "secretsd Collection.CreateItem"
            );
        }

        let item_path = item_path(&self.id, &item_id)
            .ok_or_else(|| fdo::Error::Failed("invalid item path".into()))?;

        if !replaced {
            let item = ItemInterface::new(
                item_id,
                self.id.clone(),
                self.store.clone(),
                self.channel.clone(),
            );
            if let Err(e) = object_server.at(&item_path, item).await {
                tracing::warn!(path = %item_path, error = %e, "registering item interface failed");
            }
        }

        Ok((item_path, empty_path()))
    }

    #[zbus(property)]
    async fn items(&self) -> fdo::Result<Vec<ObjectPath<'_>>> {
        let store = self.store.read().await;
        Ok(store
            .list_items(&self.id)
            .into_iter()
            .filter_map(|item_id| item_path(&self.id, item_id))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dbus_service::test_support::{FakeChannel, item_meta_json};
    use std::collections::HashMap;

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
