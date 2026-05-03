use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use zbus::{fdo, interface, zvariant::ObjectPath};

use underwrap_secretsd_store::SecretStore;

use crate::channel::{self, ChannelOps};

use super::{Secret, empty_path};

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
        channel::save_collection_meta(&*self.channel, &self.collection_id, &store)
            .map_err(|e| fdo::Error::Failed(format!("save error: {e}")))?;

        Ok(empty_path())
    }

    async fn get_secret(&self, session: ObjectPath<'_>) -> fdo::Result<Secret> {
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
        let mut store = self.store.write().await;
        if let Some(item) = store
            .collections
            .get_mut(&self.collection_id)
            .and_then(|c| c.items.get_mut(&self.id))
        {
            item.modified = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dbus_service::test_support::{FakeChannel, item_meta_json};

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
    async fn set_secret_bumps_modified_timestamp() {
        let mut store = SecretStore::new_default();
        let item_id = store
            .create_item(
                "default",
                "Item",
                &HashMap::new(),
                b"original",
                "text/plain",
                false,
            )
            .unwrap()
            .0;
        // Force an old timestamp so we can detect the bump.
        store
            .collections
            .get_mut("default")
            .unwrap()
            .items
            .get_mut(&item_id)
            .unwrap()
            .modified = 1;

        let channel: Arc<dyn ChannelOps> = Arc::new(FakeChannel::new(HashMap::from([
            (format!("default/{item_id}.meta"), item_meta_json()),
            (format!("default/{item_id}.secret"), b"original".to_vec()),
        ])));
        let store = Arc::new(RwLock::new(store));
        let item = ItemInterface::new(
            item_id.clone(),
            "default".to_string(),
            store.clone(),
            channel,
        );

        let secret = super::super::Secret {
            session: ObjectPath::try_from("/").unwrap().into(),
            parameters: vec![],
            value: b"updated".to_vec(),
            content_type: "text/plain".to_string(),
        };
        item.set_secret(secret).await.unwrap();

        let modified = store
            .read()
            .await
            .get_modified("default", &item_id)
            .unwrap();
        assert!(
            modified > 1,
            "modified timestamp was not bumped: {modified}"
        );
    }
}
