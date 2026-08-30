use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use zbus::{fdo, interface, zvariant::ObjectPath};

use underwrap_secretsd_store::SecretStore;
use zeroize::Zeroize;

use crate::metadata_trace_enabled;
use crate::publication::SharedPublisher;

use super::{Secret, empty_path, mutation::mutate_and_publish};

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
        mutate_and_publish(&mut store, &self.publisher, |candidate| {
            if let Some(item) = candidate
                .collections
                .get_mut(&self.collection_id)
                .and_then(|collection| collection.items.get_mut(&self.id))
            {
                item.secret.zeroize();
            }
            candidate.delete_item(&self.collection_id, &self.id);
            Ok(())
        })?;
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
        let mut store = self.store.write().await;
        mutate_and_publish(&mut store, &self.publisher, |candidate| {
            if let Some(item) = candidate
                .collections
                .get_mut(&self.collection_id)
                .and_then(|c| c.items.get_mut(&self.id))
            {
                item.secret.zeroize();
            }
            candidate.set_secret(
                &self.collection_id,
                &self.id,
                &secret.value,
                &secret.content_type,
            );
            Ok(())
        })?;
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
        mutate_and_publish(&mut store, &self.publisher, |candidate| {
            candidate.set_attributes(&self.collection_id, &self.id, attributes.clone());
            Ok(())
        })?;
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
        mutate_and_publish(&mut store, &self.publisher, |candidate| {
            candidate.set_label(&self.collection_id, &self.id, &label);
            Ok(())
        })?;
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
