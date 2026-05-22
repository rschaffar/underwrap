use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use zbus::{
    fdo, interface,
    zvariant::{ObjectPath, Value},
};

use underwrap_secretsd_store::SecretStore;

use crate::metadata_trace_enabled;
use crate::state;

use super::{ItemInterface, Secret, empty_path, item_path};

pub struct CollectionInterface {
    id: String,
    store: Arc<RwLock<SecretStore>>,
    state_path: Arc<PathBuf>,
}

impl CollectionInterface {
    pub fn new(id: String, store: Arc<RwLock<SecretStore>>, state_path: Arc<PathBuf>) -> Self {
        Self {
            id,
            store,
            state_path,
        }
    }
}

#[interface(name = "org.freedesktop.Secret.Collection")]
impl CollectionInterface {
    async fn delete(&self) -> fdo::Result<ObjectPath<'_>> {
        let mut store = self.store.write().await;
        store.delete_collection(&self.id);
        state::dump_or_log(&store, &self.state_path)?;
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
                .create_item(
                    &self.id,
                    &label,
                    &attrs,
                    &secret.value,
                    &secret.content_type,
                    replace,
                )
                .map_err(|e| fdo::Error::Failed(format!("create_item error: {e}")))?;
            state::dump_or_log(&store, &self.state_path)?;
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
                self.state_path.clone(),
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
        state::dump_or_log(&store, &self.state_path)?;
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
mod tests;
