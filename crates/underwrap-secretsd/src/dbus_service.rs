//! D-Bus Secret Service interfaces (org.freedesktop.Secret.Service,
//! .Collection, .Item, .Session).
//!
//! Only "plain" sessions are supported — all communication is over a
//! local Unix socket inside a PID namespace, so DH encryption is unnecessary.

#[path = "dbus_service/collection.rs"]
mod collection;
#[path = "dbus_service/item.rs"]
mod item;
#[path = "dbus_service/service.rs"]
mod service;

pub use collection::CollectionInterface;
pub use item::ItemInterface;
pub use service::SecretService;

use zbus::{
    fdo, interface,
    zvariant::{ObjectPath, OwnedObjectPath},
};
use zeroize::Zeroize;

use crate::BASE_PATH;

/// Secret type for the D-Bus protocol.
#[derive(Debug, serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
pub struct Secret {
    pub session: OwnedObjectPath,
    pub parameters: Vec<u8>,
    pub value: Vec<u8>,
    pub content_type: String,
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

pub struct SessionInterface;

#[interface(name = "org.freedesktop.Secret.Session")]
impl SessionInterface {
    async fn close(&self) -> fdo::Result<()> {
        Ok(())
    }
}

pub(crate) fn collection_path(col_id: &str) -> Result<ObjectPath<'static>, fdo::Error> {
    ObjectPath::try_from(format!("{BASE_PATH}/collection/{col_id}"))
        .map_err(|e| fdo::Error::Failed(format!("invalid path: {e}")))
}

pub(crate) fn item_path(col_id: &str, item_id: &str) -> Option<ObjectPath<'static>> {
    ObjectPath::try_from(format!("{BASE_PATH}/collection/{col_id}/{item_id}")).ok()
}

pub(crate) fn empty_path() -> ObjectPath<'static> {
    ObjectPath::try_from("/").unwrap()
}

pub(crate) fn parse_item_path(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix(&format!("{BASE_PATH}/collection/"))?;
    let (col, item) = rest.split_once('/')?;
    if col.is_empty() || item.is_empty() {
        None
    } else {
        Some((col, item))
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::sync::RwLock;

    use underwrap_secretsd_store::SecretStore;

    use super::SecretService;
    use crate::channel::{ChannelOps, ItemMeta};

    pub(crate) struct FakeChannel {
        pub(crate) data: StdMutex<HashMap<String, Vec<u8>>>,
        fail_delete: HashSet<String>,
    }

    impl FakeChannel {
        pub(crate) fn new(data: HashMap<String, Vec<u8>>) -> Self {
            Self {
                data: StdMutex::new(data),
                fail_delete: HashSet::new(),
            }
        }

        pub(crate) fn with_failing_delete(mut self, key: &str) -> Self {
            self.fail_delete.insert(key.to_string());
            self
        }
    }

    impl ChannelOps for FakeChannel {
        fn read(
            &self,
            secret_id: &str,
        ) -> anyhow::Result<Option<underwrap_protocol::message::SecretBytes>> {
            Ok(self
                .data
                .lock()
                .unwrap()
                .get(secret_id)
                .cloned()
                .map(Into::into))
        }

        fn write(&self, secret_id: &str, material: &[u8]) -> anyhow::Result<()> {
            self.data
                .lock()
                .unwrap()
                .insert(secret_id.to_string(), material.to_vec());
            Ok(())
        }

        fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
            let prefix = format!("{prefix}/");
            Ok(self
                .data
                .lock()
                .unwrap()
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .cloned()
                .collect())
        }

        fn delete(&self, secret_id: &str) -> anyhow::Result<bool> {
            if self.fail_delete.contains(secret_id) {
                anyhow::bail!("forced delete failure for {secret_id}");
            }
            Ok(self.data.lock().unwrap().remove(secret_id).is_some())
        }
    }

    pub(crate) fn item_meta_json() -> Vec<u8> {
        serde_json::to_vec(&ItemMeta {
            content_type: "text/plain".to_string(),
            attributes: HashMap::new(),
            label: "Item".to_string(),
            created: 1,
            modified: 1,
        })
        .unwrap()
    }

    pub(crate) fn service_with_channel(
        store: SecretStore,
        channel: Arc<dyn ChannelOps>,
    ) -> (SecretService, Arc<RwLock<SecretStore>>) {
        let store = Arc::new(RwLock::new(store));
        let service = SecretService::new(store.clone(), channel);
        (service, store)
    }
}

#[cfg(test)]
mod tests {
    use super::parse_item_path;
    use crate::BASE_PATH;

    #[test]
    fn parse_item_path_accepts_valid_secret_service_paths() {
        assert_eq!(
            parse_item_path(&format!("{BASE_PATH}/collection/default/item_1")),
            Some(("default", "item_1"))
        );
        assert_eq!(parse_item_path("/invalid"), None);
        assert_eq!(
            parse_item_path(&format!("{BASE_PATH}/collection/default")),
            None
        );
    }
}
