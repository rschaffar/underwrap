use super::*;
use std::collections::HashMap;
use underwrap_protocol::resource_blob::LibsecretSection;
use underwrap_secretsd_store::{Collection as StoreCollection, Item as StoreItem};

fn populated_store() -> SecretStore {
    let mut store = SecretStore::new_default();
    store.collections.clear();
    store.aliases.clear();

    let mut items = HashMap::new();
    items.insert(
        "gh-cli".to_string(),
        StoreItem {
            secret: b"ghp_secret".to_vec(),
            content_type: "text/plain".to_string(),
            attributes: HashMap::from([("server".to_string(), "github.com".to_string())]),
            label: "gh CLI".to_string(),
            created: 100,
            modified: 200,
        },
    );
    store.collections.insert(
        "default".to_string(),
        StoreCollection {
            label: "Default".to_string(),
            items,
            created: 10,
            modified: 20,
        },
    );
    store
        .aliases
        .insert("default".to_string(), "default".to_string());
    store
}

#[test]
fn dump_writes_canonical_bytes_atomically() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let path = tempdir.path().join("libsecret-state.jcs");

    dump(&populated_store(), &path).unwrap();

    let read = std::fs::read(&path).unwrap();
    let section = LibsecretSection::from_canonical_bytes(&read).unwrap();
    assert!(section.collections.contains_key("default"));
}

#[test]
fn dump_replaces_existing_file_atomically() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let path = tempdir.path().join("libsecret-state.jcs");

    std::fs::write(&path, b"stale").unwrap();
    dump(&populated_store(), &path).unwrap();

    let read = std::fs::read(&path).unwrap();
    assert_ne!(read, b"stale");
    let _ = LibsecretSection::from_canonical_bytes(&read).unwrap();
}
