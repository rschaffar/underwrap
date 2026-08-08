use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use zbus::{fdo, interface, zvariant::ObjectPath};

use underwrap_secretsd_store::SecretStore;

use crate::metadata_trace_enabled;
use crate::publication::{self, SharedPublisher};

use super::{Secret, empty_path};

pub struct ItemInterface {
    id: String,
    collection_id: String,
    store: Arc<RwLock<SecretStore>>,
    publisher: SharedPublisher,
}

impl ItemInterface {
    pub fn new(
        id: String,
        collection_id: String,
        store: Arc<RwLock<SecretStore>>,
        publisher: SharedPublisher,
    ) -> Self {
        Self {
            id,
            collection_id,
            store,
            publisher,
        }
    }
}

#[interface(name = "org.freedesktop.Secret.Item")]
impl ItemInterface {
    async fn delete(&self) -> fdo::Result<ObjectPath<'_>> {
        let mut store = self.store.write().await;
        store.delete_item(&self.collection_id, &self.id);
        publication::publish_store(&self.publisher, &store)?;
        Ok(empty_path())
    }

    async fn get_secret(&self, session: ObjectPath<'_>) -> fdo::Result<(Secret,)> {
        let store = self.store.read().await;
        let item = store
            .collections
            .get(&self.collection_id)
            .and_then(|c| c.items.get(&self.id))
            .ok_or_else(|| fdo::Error::Failed("item not found".into()))?;

        let content_type = item.content_type.clone();
        let secret = item.secret.clone();

        if metadata_trace_enabled() {
            tracing::info!(
                collection = %self.collection_id,
                item = %self.id,
                content_type = %content_type,
                secret_len = secret.len(),
                "secretsd Item.GetSecret"
            );
        }

        Ok((Secret {
            session: session.to_owned().into(),
            parameters: vec![],
            value: secret,
            content_type,
        },))
    }

    async fn set_secret(&self, secret: Secret) -> fdo::Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut store = self.store.write().await;
        if let Some(item) = store
            .collections
            .get_mut(&self.collection_id)
            .and_then(|c| c.items.get_mut(&self.id))
        {
            item.secret = secret.value.clone();
            item.content_type = secret.content_type.clone();
            item.modified = now;
        }
        publication::publish_store(&self.publisher, &store)?;
        if metadata_trace_enabled() {
            tracing::info!(
                collection = %self.collection_id,
                item = %self.id,
                content_type = %secret.content_type,
                secret_len = secret.value.len(),
                "secretsd Item.SetSecret"
            );
        }
        Ok(())
    }

    #[zbus(property)]
    async fn locked(&self) -> bool {
        false
    }

    #[zbus(property)]
    async fn attributes(&self) -> fdo::Result<HashMap<String, String>> {
        let store = self.store.read().await;
        let attributes = store
            .get_attributes(&self.collection_id, &self.id)
            .cloned()
            .unwrap_or_default();
        if metadata_trace_enabled() {
            tracing::info!(
                collection = %self.collection_id,
                item = %self.id,
                ?attributes,
                "secretsd Item.Attributes"
            );
        }
        Ok(attributes)
    }

    #[zbus(property)]
    async fn set_attributes(&mut self, attributes: HashMap<String, String>) -> fdo::Result<()> {
        let mut store = self.store.write().await;
        store.set_attributes(&self.collection_id, &self.id, attributes.clone());
        publication::publish_store(&self.publisher, &store)?;
        if metadata_trace_enabled() {
            tracing::info!(
                collection = %self.collection_id,
                item = %self.id,
                ?attributes,
                "secretsd Item.SetAttributes"
            );
        }
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
        publication::publish_store(&self.publisher, &store)?;
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
mod tests;
