use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use zbus::{
    fdo, interface,
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value},
};

use underwrap_secretsd_store::SecretStore;

use crate::BASE_PATH;
use crate::metadata_trace_enabled;
use crate::publication::{self, SharedPublisher};

use super::{
    CollectionInterface, Secret, SessionInterface, collection_path, empty_path, item_path,
    parse_item_path,
};

pub struct SecretService {
    store: Arc<RwLock<SecretStore>>,
    publisher: SharedPublisher,
}

impl SecretService {
    pub fn new(store: Arc<RwLock<SecretStore>>, publisher: SharedPublisher) -> Self {
        Self { store, publisher }
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

        // The Label is the collection name and becomes its object-path
        // segment (see `SecretStore::create_collection`). It is untrusted
        // input from the wrapped target flowing into a D-Bus object path,
        // so it must already be a valid path element — reject, never
        // sanitize, and reject *before* touching the store so an illegal
        // name cannot persist an unreachable collection.
        let name = label.as_deref().unwrap_or("");
        validate_collection_name(name).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;

        let alias = if alias.is_empty() { None } else { Some(alias) };

        if metadata_trace_enabled() {
            tracing::info!(
                label = label.as_deref().unwrap_or(""),
                alias = alias.unwrap_or(""),
                "secretsd CreateCollection"
            );
        }

        let col_id = {
            let mut store = self.store.write().await;
            let id = store.create_collection(name, alias);
            publication::publish_store(&self.publisher, &store)?;
            id
        };

        let path = collection_path(&col_id)?;
        let col = CollectionInterface::new(col_id, self.store.clone(), self.publisher.clone());
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
        if metadata_trace_enabled() {
            tracing::info!(
                ?attributes,
                result_count = paths.len(),
                "secretsd Service.SearchItems"
            );
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
                && let Some(item) = store
                    .collections
                    .get(col_id)
                    .and_then(|c| c.items.get(item_id))
            {
                let content_type = item.content_type.clone();
                let secret = item.secret.clone();
                if metadata_trace_enabled() {
                    tracing::info!(
                        collection = col_id,
                        item = item_id,
                        content_type = %content_type,
                        secret_len = secret.len(),
                        "secretsd Service.GetSecrets item"
                    );
                }
                results.insert(
                    item_path.into(),
                    Secret {
                        session: session.to_owned().into(),
                        parameters: vec![],
                        value: secret,
                        content_type,
                    },
                );
            }
        }

        if metadata_trace_enabled() {
            tracing::info!(result_count = results.len(), "secretsd Service.GetSecrets");
        }

        Ok(results)
    }

    async fn read_alias(&self, name: &str) -> fdo::Result<ObjectPath<'_>> {
        let store = self.store.read().await;
        let path = if let Some(col_id) = store.get_alias(name) {
            collection_path(col_id)?
        } else {
            empty_path()
        };
        if metadata_trace_enabled() {
            tracing::info!(alias = name, path = %path, "secretsd Service.ReadAlias");
        }
        Ok(path)
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
        if metadata_trace_enabled() {
            tracing::info!(
                alias = name,
                collection = col_id.as_deref().unwrap_or(""),
                "secretsd Service.SetAlias"
            );
        }
        publication::publish_store(&self.publisher, &store)?;
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

/// Upper bound on a collection name's length. A D-Bus path element must
/// fit, with room left for the fixed prefix and a trailing item segment.
/// Matches the backend admin id cap.
const MAX_COLLECTION_NAME_LEN: usize = 128;

/// Validate a client-requested collection name before it is used as a
/// D-Bus object-path segment. A path element may contain only
/// `[A-Za-z0-9_]` and must be non-empty (a leading digit is legal for
/// object paths). We reject anything else rather than sanitizing it:
/// the name is untrusted input from the wrapped target, and silently
/// mapping `foo/bar` or `""` into a path would invite element injection
/// (collision with `/collection/<col>/<item>` item paths) or an invalid
/// path. All real clients already use valid names (`gogcli`,
/// `awsssocli`, `login`, `default`), so this never rejects a real one.
fn validate_collection_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("collection name must not be empty");
    }
    if name.len() > MAX_COLLECTION_NAME_LEN {
        return Err("collection name too long");
    }
    if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err("collection name must match [A-Za-z0-9_]");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
