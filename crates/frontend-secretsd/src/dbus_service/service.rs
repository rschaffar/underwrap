use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use zbus::{
    fdo, interface,
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value},
};

use underwrap_secretsd_store::SecretStore;

use crate::BASE_PATH;
use crate::channel::{self, ChannelOps};

use super::{
    CollectionInterface, Secret, SessionInterface, collection_path, empty_path, item_path,
    parse_item_path,
};

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

        if let Err(e) = object_server.at(&path, SessionInterface).await {
            tracing::warn!(%path, error = %e, "registering session interface failed");
        }

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

        let path = collection_path(&col_id)?;
        let col = CollectionInterface::new(col_id, self.store.clone(), self.channel.clone());
        if let Err(e) = object_server.at(&path, col).await {
            tracing::warn!(%path, error = %e, "registering collection interface failed");
        }

        Ok((path, empty_path()))
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
                if let Some(path) = item_path(col_id, item_id) {
                    paths.push(path);
                }
            }
        }
        Ok((paths, vec![]))
    }

    async fn unlock(
        &self,
        objects: Vec<OwnedObjectPath>,
    ) -> fdo::Result<(Vec<OwnedObjectPath>, ObjectPath<'_>)> {
        Ok((objects, empty_path()))
    }

    async fn lock(
        &self,
        _objects: Vec<OwnedObjectPath>,
    ) -> fdo::Result<(Vec<ObjectPath<'_>>, ObjectPath<'_>)> {
        Ok((vec![], empty_path()))
    }

    async fn get_secrets(
        &self,
        items: Vec<ObjectPath<'_>>,
        session: ObjectPath<'_>,
    ) -> fdo::Result<HashMap<OwnedObjectPath, Secret>> {
        let store = self.store.read().await;
        let mut results = HashMap::new();

        for item_path in items {
            if let Some((col_id, item_id)) = parse_item_path(item_path.as_str())
                && let Ok(Some(secret)) = channel::read_item_secret(&*self.channel, col_id, item_id)
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

        Ok(results)
    }

    async fn read_alias(&self, name: &str) -> fdo::Result<ObjectPath<'_>> {
        let store = self.store.read().await;
        if let Some(col_id) = store.get_alias(name) {
            collection_path(col_id)
        } else {
            Ok(empty_path())
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
            .filter_map(|id| collection_path(id).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dbus_service::test_support::{FakeChannel, service_with_channel};

    #[tokio::test]
    async fn read_and_set_alias_roundtrip() {
        let mut store = SecretStore::new_default();
        let work_id = store.create_collection(Some("Work"), None);
        let channel_impl = Arc::new(FakeChannel::new(HashMap::new()));
        let channel: Arc<dyn ChannelOps> = channel_impl.clone();
        let (service, store) = service_with_channel(store, channel);

        let path = service.read_alias("default").await.unwrap();
        assert_eq!(path.as_str(), format!("{BASE_PATH}/collection/default"));

        let work_path = collection_path(&work_id).unwrap();
        service.set_alias("work", work_path).await.unwrap();

        let alias_path = service.read_alias("work").await.unwrap();
        assert_eq!(
            alias_path.as_str(),
            format!("{BASE_PATH}/collection/{work_id}")
        );
        assert_eq!(store.read().await.get_alias("work"), Some(work_id.as_str()));

        let raw_aliases = channel_impl
            .data
            .lock()
            .unwrap()
            .get("_aliases")
            .cloned()
            .unwrap();
        let aliases: HashMap<String, String> = serde_json::from_slice(&raw_aliases).unwrap();
        assert_eq!(aliases.get("work"), Some(&work_id));
    }

    #[tokio::test]
    async fn search_items_and_get_secrets_use_store_metadata_and_channel_values() {
        let mut store = SecretStore::new_default();
        let attributes = HashMap::from([("service".to_string(), "github".to_string())]);
        let (item_id, _) = store
            .create_item(
                "default",
                "GitHub",
                &attributes,
                b"ignored-by-service",
                "text/plain",
                false,
            )
            .unwrap();

        let channel: Arc<dyn ChannelOps> = Arc::new(FakeChannel::new(HashMap::from([(
            format!("default/{item_id}.secret"),
            b"backend-secret".to_vec(),
        )])));
        let (service, _) = service_with_channel(store, channel);

        let (unlocked, locked) = service.search_items(attributes).await.unwrap();
        assert!(locked.is_empty());
        assert_eq!(unlocked.len(), 1);
        let item_path_str = unlocked[0].as_str().to_string();
        assert_eq!(
            item_path_str,
            format!("{BASE_PATH}/collection/default/{item_id}")
        );

        let session = ObjectPath::try_from(format!("{BASE_PATH}/session/s0")).unwrap();
        let secrets = service
            .get_secrets(
                vec![ObjectPath::try_from(item_path_str.as_str()).unwrap()],
                session,
            )
            .await
            .unwrap();
        let secret = secrets
            .get(&OwnedObjectPath::try_from(item_path_str.clone()).unwrap())
            .unwrap();
        assert_eq!(secret.value, b"backend-secret");
        assert_eq!(secret.content_type, "text/plain");
    }
}
