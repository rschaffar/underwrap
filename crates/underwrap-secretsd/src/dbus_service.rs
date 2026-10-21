//! D-Bus Secret Service interfaces (org.freedesktop.Secret.Service,
//! .Collection, .Item, .Session).
//!
//! Only "plain" sessions are supported — all communication is over a
//! local Unix socket inside a PID namespace, so DH encryption is unnecessary.

mod collection;
mod exports;
mod item;
mod mutation;
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

pub(crate) async fn register_objects(
    object_server: &zbus::ObjectServer,
    store: &std::sync::Arc<tokio::sync::RwLock<underwrap_secretsd_store::SecretStore>>,
    publisher: &crate::publication::SharedPublisher,
) -> fdo::Result<()> {
    let initial = exports::Exports::from_store(&*store.read().await);
    exports::Exports::default()
        .reconcile(&initial, object_server, store, publisher)
        .await
}

pub(crate) fn alias_path(name: &str) -> Result<ObjectPath<'static>, fdo::Error> {
    ObjectPath::try_from(format!("{BASE_PATH}/aliases/{name}"))
        .map_err(|_| fdo::Error::Failed("invalid alias path".into()))
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
mod tests;
#[cfg(test)]
mod wire_tests;
