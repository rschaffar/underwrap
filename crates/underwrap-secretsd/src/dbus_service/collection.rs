use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use zbus::{
    fdo, interface,
    zvariant::{ObjectPath, Value},
};

use underwrap_secretsd_store::SecretStore;
use zeroize::Zeroize;

use crate::metadata_trace_enabled;
use crate::publication::SharedPublisher;

use super::{ItemInterface, Secret, empty_path, item_path, mutation::mutate_and_publish};

const ITEM_LABEL_PROPERTY: &str = "org.freedesktop.Secret.Item.Label";
const ITEM_ATTRIBUTES_PROPERTY: &str = "org.freedesktop.Secret.Item.Attributes";
const ITEM_LABEL_TYPE_ERROR: &str = "org.freedesktop.Secret.Item.Label must have D-Bus type s";
const ITEM_ATTRIBUTES_TYPE_ERROR: &str =
    "org.freedesktop.Secret.Item.Attributes must have D-Bus type a{ss}";

fn parse_item_label(properties: &HashMap<String, Value<'_>>) -> fdo::Result<String> {
    let Some(label) = properties.get(ITEM_LABEL_PROPERTY) else {
        return Ok(String::new());
    };
    if label.value_signature() != "s" {
        return Err(fdo::Error::InvalidArgs(ITEM_LABEL_TYPE_ERROR.into()));
    }
    label
        .downcast_ref::<String>()
        .map_err(|_| fdo::Error::InvalidArgs(ITEM_LABEL_TYPE_ERROR.into()))
}

fn parse_item_attributes(
    properties: &HashMap<String, Value<'_>>,
) -> fdo::Result<HashMap<String, String>> {
    let Some(attributes) = properties.get(ITEM_ATTRIBUTES_PROPERTY) else {
        return Ok(HashMap::new());
    };
    if attributes.value_signature() != "a{ss}" {
        return Err(fdo::Error::InvalidArgs(ITEM_ATTRIBUTES_TYPE_ERROR.into()));
    }
    let dict = attributes
        .downcast_ref::<zbus::zvariant::Dict>()
        .map_err(|_| fdo::Error::InvalidArgs(ITEM_ATTRIBUTES_TYPE_ERROR.into()))?;
    HashMap::<String, String>::try_from(dict)
        .map_err(|_| fdo::Error::InvalidArgs(ITEM_ATTRIBUTES_TYPE_ERROR.into()))
}

pub struct CollectionInterface {
    id: String,
    store: Arc<RwLock<SecretStore>>,
    publisher: SharedPublisher,
}

impl CollectionInterface {
    pub fn new(id: String, store: Arc<RwLock<SecretStore>>, publisher: SharedPublisher) -> Self {
        Self {
            id,
            store,
            publisher,
        }
    }
}

#[interface(name = "org.freedesktop.Secret.Collection")]
impl CollectionInterface {
    async fn delete(&self) -> fdo::Result<ObjectPath<'_>> {
        let mut store = self.store.write().await;
        mutate_and_publish(&mut store, &self.publisher, |candidate| {
            if let Some(collection) = candidate.collections.get_mut(&self.id) {
                for item in collection.items.values_mut() {
                    item.secret.zeroize();
                }
            }
            candidate.delete_collection(&self.id);
            Ok(())
        })?;
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
        let label = parse_item_label(&properties)?;
        let attrs = parse_item_attributes(&properties)?;

        let secret_len = secret.value.len();
        let content_type = secret.content_type.clone();
        let (item_id, replaced, item_path) = {
            let mut store = self.store.write().await;
            mutate_and_publish(&mut store, &self.publisher, |candidate| {
                if replace
                    && let Some(collection) = candidate.collections.get_mut(&self.id)
                    && let Some(item) = collection.items.values_mut().find(|item| {
                        attrs
                            .iter()
                            .all(|(key, value)| item.attributes.get(key) == Some(value))
                    })
                {
                    item.secret.zeroize();
                }
                let (id, replaced) = candidate
                    .create_item(
                        &self.id,
                        &label,
                        &attrs,
                        &secret.value,
                        &secret.content_type,
                        replace,
                    )
                    .map_err(|e| fdo::Error::Failed(format!("create_item error: {e}")))?;
                let path = item_path(&self.id, &id)
                    .ok_or_else(|| fdo::Error::Failed("invalid item path".into()))?;
                Ok((id, replaced, path))
            })?
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

        if !replaced {
            let item = ItemInterface::new(
                item_id,
                self.id.clone(),
                self.store.clone(),
                self.publisher.clone(),
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
        mutate_and_publish(&mut store, &self.publisher, |candidate| {
            candidate.set_collection_label(&self.id, &label);
            Ok(())
        })?;
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
