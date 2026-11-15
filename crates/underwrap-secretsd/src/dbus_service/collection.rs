use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use zbus::{ObjectPath, Variant, fdo, interface};

use underwrap_secretsd_store::SecretStore;
use zeroize::Zeroize;

use crate::metadata_trace_enabled;
use crate::publication::SharedPublisher;

use super::{
    Secret, empty_path, item_path,
    mutation::{mutate_and_export, mutate_and_publish},
};

const ITEM_LABEL_PROPERTY: &str = "org.freedesktop.Secret.Item.Label";
const ITEM_ATTRIBUTES_PROPERTY: &str = "org.freedesktop.Secret.Item.Attributes";
const ITEM_LABEL_TYPE_ERROR: &str = "org.freedesktop.Secret.Item.Label must have D-Bus type s";
const ITEM_ATTRIBUTES_TYPE_ERROR: &str =
    "org.freedesktop.Secret.Item.Attributes must have D-Bus type a{ss}";

fn parse_item_label(properties: &HashMap<String, Variant<'_>>) -> fdo::Result<String> {
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
    properties: &HashMap<String, Variant<'_>>,
) -> fdo::Result<HashMap<String, String>> {
    let Some(attributes) = properties.get(ITEM_ATTRIBUTES_PROPERTY) else {
        return Ok(HashMap::new());
    };
    if attributes.value_signature() != "a{ss}" {
        return Err(fdo::Error::InvalidArgs(ITEM_ATTRIBUTES_TYPE_ERROR.into()));
    }
    let dict = attributes
        .downcast_ref::<zbus::Dict>()
        .map_err(|_| fdo::Error::InvalidArgs(ITEM_ATTRIBUTES_TYPE_ERROR.into()))?;
    HashMap::<String, String>::try_from(dict)
        .map_err(|_| fdo::Error::InvalidArgs(ITEM_ATTRIBUTES_TYPE_ERROR.into()))
}

enum CollectionSelector {
    Id(String),
    Alias(String),
}

pub struct CollectionInterface {
    selector: CollectionSelector,
    store: Arc<RwLock<SecretStore>>,
    publisher: SharedPublisher,
}

impl CollectionInterface {
    pub fn new(id: String, store: Arc<RwLock<SecretStore>>, publisher: SharedPublisher) -> Self {
        Self {
            selector: CollectionSelector::Id(id),
            store,
            publisher,
        }
    }

    pub fn alias(
        name: String,
        store: Arc<RwLock<SecretStore>>,
        publisher: SharedPublisher,
    ) -> Self {
        Self {
            selector: CollectionSelector::Alias(name),
            store,
            publisher,
        }
    }

    fn resolve<'a>(&'a self, store: &'a SecretStore) -> fdo::Result<&'a str> {
        let id = match &self.selector {
            CollectionSelector::Id(id) => id.as_str(),
            CollectionSelector::Alias(name) => store
                .get_alias(name)
                .ok_or_else(|| fdo::Error::UnknownObject("collection not found".into()))?,
        };
        if !store.collections.contains_key(id) {
            return Err(fdo::Error::UnknownObject("collection not found".into()));
        }
        Ok(id)
    }
}

#[interface(name = "org.freedesktop.Secret.Collection")]
impl CollectionInterface {
    pub(super) async fn delete(
        &self,
        #[zbus(object_server)] object_server: &zbus::ObjectServer,
    ) -> fdo::Result<ObjectPath<'_>> {
        mutate_and_export(&self.store, &self.publisher, object_server, |candidate| {
            let id = self.resolve(candidate)?.to_owned();
            if let Some(collection) = candidate.collections.get_mut(&id) {
                for item in collection.items.values_mut() {
                    item.secret.zeroize();
                }
            }
            candidate.delete_collection(&id);
            Ok(())
        })
        .await?;
        Ok(empty_path())
    }

    async fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> fdo::Result<Vec<ObjectPath<'_>>> {
        let store = self.store.read().await;
        let id = self.resolve(&store)?;
        let paths: Vec<_> = store
            .search(id, &attributes)
            .into_iter()
            .filter_map(|item_id| item_path(id, item_id))
            .collect();
        if metadata_trace_enabled() {
            tracing::info!(
                collection = id,
                ?attributes,
                result_count = paths.len(),
                "secretsd Collection.SearchItems"
            );
        }
        Ok(paths)
    }

    async fn create_item(
        &self,
        properties: HashMap<String, Variant<'_>>,
        secret: Secret,
        replace: bool,
        #[zbus(object_server)] object_server: &zbus::ObjectServer,
    ) -> fdo::Result<(ObjectPath<'_>, ObjectPath<'_>)> {
        let label = parse_item_label(&properties)?;
        let attrs = parse_item_attributes(&properties)?;
        let (id, item_id, replaced, path) =
            mutate_and_export(&self.store, &self.publisher, object_server, |candidate| {
                let id = self.resolve(candidate)?.to_owned();
                if replace
                    && let Some(collection) = candidate.collections.get_mut(&id)
                    && let Some(item) = collection.items.values_mut().find(|item| {
                        attrs
                            .iter()
                            .all(|(key, value)| item.attributes.get(key) == Some(value))
                    })
                {
                    item.secret.zeroize();
                }
                let (item_id, replaced) = candidate
                    .create_item(
                        &id,
                        &label,
                        &attrs,
                        &secret.value,
                        &secret.content_type,
                        replace,
                    )
                    .map_err(|e| fdo::Error::Failed(format!("create_item error: {e}")))?;
                let path = item_path(&id, &item_id)
                    .ok_or_else(|| fdo::Error::Failed("invalid item path".into()))?;
                Ok((id, item_id, replaced, path))
            })
            .await?;
        if metadata_trace_enabled() {
            tracing::info!(
                collection = id,
                item = item_id,
                label,
                ?attrs,
                replace,
                replaced,
                content_type = secret.content_type,
                secret_len = secret.value.len(),
                "secretsd Collection.CreateItem"
            );
        }
        Ok((path, empty_path()))
    }

    #[zbus(property)]
    async fn items(&self) -> fdo::Result<Vec<ObjectPath<'_>>> {
        let store = self.store.read().await;
        let id = self.resolve(&store)?;
        Ok(store
            .list_items(id)
            .into_iter()
            .filter_map(|item_id| item_path(id, item_id))
            .collect())
    }

    #[zbus(property)]
    async fn label(&self) -> fdo::Result<String> {
        let store = self.store.read().await;
        Ok(store
            .get_collection_label(self.resolve(&store)?)
            .unwrap_or("")
            .to_string())
    }

    // Interior mutation needs no exclusive interface lock. zbus Properties.Set
    // holds a registry read while acquiring this handler; an exclusive lock could
    // wait on a graph method that is itself waiting to reconcile the registry.
    #[zbus(property)]
    pub(super) async fn set_label(&self, label: String) -> fdo::Result<()> {
        let mut store = self.store.write().await;
        let id = self.resolve(&store)?.to_owned();
        mutate_and_publish(&mut store, &self.publisher, |candidate| {
            candidate.set_collection_label(&id, &label);
            Ok(())
        })
    }

    #[zbus(property)]
    async fn locked(&self) -> fdo::Result<bool> {
        self.resolve(&*self.store.read().await)?;
        Ok(false)
    }

    #[zbus(property)]
    async fn created(&self) -> fdo::Result<u64> {
        let store = self.store.read().await;
        Ok(store
            .get_collection_created(self.resolve(&store)?)
            .unwrap_or(0))
    }

    #[zbus(property)]
    async fn modified(&self) -> fdo::Result<u64> {
        let store = self.store.read().await;
        Ok(store
            .get_collection_modified(self.resolve(&store)?)
            .unwrap_or(0))
    }
}

#[cfg(test)]
mod tests;
