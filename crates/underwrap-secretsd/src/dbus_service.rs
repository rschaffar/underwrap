//! D-Bus Secret Service interfaces (org.freedesktop.Secret.Service,
//! .Collection, .Item, .Session).
//!
//! Only "plain" sessions are supported — all communication is over a
//! local Unix socket inside a PID namespace, so DH encryption is unnecessary.

mod collection;
mod item;
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
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use tokio::sync::RwLock;

    use underwrap_protocol::libsecret_publication::{
        REQUEST_HEADER_LEN, ResponseKind, decode_request_header, encode_response,
    };
    use underwrap_protocol::resource_blob::LibsecretSection;
    use underwrap_secretsd_store::SecretStore;

    use super::SecretService;
    use crate::publication::{Publisher, SharedPublisher};

    type ServiceHarness = (
        SecretService,
        Arc<RwLock<SecretStore>>,
        SharedPublisher,
        Arc<Mutex<Vec<LibsecretSection>>>,
        JoinHandle<()>,
    );

    pub(crate) fn service_with_publisher(store: SecretStore) -> ServiceHarness {
        let (publisher_stream, mut host) = UnixStream::pair().unwrap();
        let published = Arc::new(Mutex::new(Vec::new()));
        let observed = published.clone();
        let responder = std::thread::spawn(move || {
            loop {
                let mut header = [0; REQUEST_HEADER_LEN];
                if host.read_exact(&mut header).is_err() {
                    break;
                }
                let header = decode_request_header(&header).unwrap();
                let mut body = vec![0; header.body_len];
                if host.read_exact(&mut body).is_err() {
                    break;
                }
                let section = LibsecretSection::from_strict_publication_bytes(&body).unwrap();
                observed.lock().unwrap().push(section);
                if host
                    .write_all(&encode_response(ResponseKind::Ack, header.generation))
                    .is_err()
                {
                    break;
                }
            }
        });
        let publisher = Arc::new(Mutex::new(Publisher::from_stream(publisher_stream)));
        let store = Arc::new(RwLock::new(store));
        let service = SecretService::new(store.clone(), publisher.clone());
        (service, store, publisher, published, responder)
    }
}

#[cfg(test)]
mod tests;
