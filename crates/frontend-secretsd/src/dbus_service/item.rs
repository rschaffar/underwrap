use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use zbus::{fdo, interface, zvariant::ObjectPath};

use underwrap_secretsd_store::SecretStore;

use crate::metadata_trace_enabled;
use crate::state;

use super::{Secret, empty_path};

pub struct ItemInterface {
    id: String,
    collection_id: String,
    store: Arc<RwLock<SecretStore>>,
    state_path: Arc<PathBuf>,
}

impl ItemInterface {
    pub fn new(
        id: String,
        collection_id: String,
        store: Arc<RwLock<SecretStore>>,
        state_path: Arc<PathBuf>,
    ) -> Self {
        Self {
            id,
            collection_id,
            store,
            state_path,
        }
    }
}

#[interface(name = "org.freedesktop.Secret.Item")]
impl ItemInterface {
    async fn delete(&self) -> fdo::Result<ObjectPath<'_>> {
        let mut store = self.store.write().await;
        store.delete_item(&self.collection_id, &self.id);
        state::dump_or_log(&store, &self.state_path)?;
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
        state::dump_or_log(&store, &self.state_path)?;
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
        state::dump_or_log(&store, &self.state_path)?;
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
        state::dump_or_log(&store, &self.state_path)?;
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
    use crate::dbus_service::test_support::service_with_state;
    use underwrap_protocol::resource_blob::LibsecretSection;

    #[tokio::test]
    async fn set_secret_bumps_modified_and_persists_state() {
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

        let (_service, store_arc, tempdir) = service_with_state(store);
        let state_path = tempdir.path().join("libsecret-state.jcs");
        let item = ItemInterface::new(
            item_id.clone(),
            "default".to_string(),
            store_arc.clone(),
            Arc::new(state_path.clone()),
        );

        let secret = super::super::Secret {
            session: ObjectPath::try_from("/").unwrap().into(),
            parameters: vec![],
            value: b"updated".to_vec(),
            content_type: "text/plain".to_string(),
        };
        item.set_secret(secret).await.unwrap();

        let store_guard = store_arc.read().await;
        let item = store_guard
            .collections
            .get("default")
            .and_then(|c| c.items.get(&item_id))
            .unwrap();
        assert!(item.modified > 1);
        assert_eq!(item.secret, b"updated");

        let bytes = std::fs::read(&state_path).unwrap();
        let section = LibsecretSection::from_canonical_bytes(&bytes).unwrap();
        let item = section
            .collections
            .get("default")
            .and_then(|c| c.items.get(&item_id))
            .unwrap();
        assert_eq!(item.secret.as_slice(), b"updated");
    }
}
