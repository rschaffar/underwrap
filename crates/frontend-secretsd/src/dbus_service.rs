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
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    use underwrap_secretsd_store::SecretStore;

    use super::SecretService;

    /// Build a `SecretService` with a temporary state path. Caller owns
    /// the `TempDir`; dropping it cleans up the file.
    pub(crate) fn service_with_state(
        store: SecretStore,
    ) -> (SecretService, Arc<RwLock<SecretStore>>, tempfile::TempDir) {
        let tempdir = tempfile::TempDir::new().unwrap();
        let state_path: Arc<PathBuf> = Arc::new(tempdir.path().join("libsecret-state.jcs"));
        // Materialize an initial state file so tests can re-read it
        // after the first mutation.
        let initial = crate::section_store::section_from_store(&store);
        std::fs::write(&*state_path, initial.to_canonical_bytes().unwrap()).unwrap();
        let store = Arc::new(RwLock::new(store));
        let service = SecretService::new(store.clone(), state_path);
        (service, store, tempdir)
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
