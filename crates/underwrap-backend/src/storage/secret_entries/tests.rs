use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use underwrap_backend_store::{StorageBackend, load_resource_blob, save_resource_blob};
use underwrap_protocol::message::SecretBytes;

use super::*;
use crate::test_support::mixed_resource_blob_for_test;

#[derive(Default)]
struct CountingStore {
    data: Mutex<HashMap<String, Vec<u8>>>,
    writes: AtomicUsize,
}

impl CountingStore {
    fn reset_write_count(&self) {
        self.writes.store(0, Ordering::SeqCst);
    }

    fn write_count(&self) -> usize {
        self.writes.load(Ordering::SeqCst)
    }
}

impl StorageBackend for CountingStore {
    fn read(&self, id: &str) -> anyhow::Result<Option<SecretBytes>> {
        Ok(self.data.lock().unwrap().get(id).cloned().map(Into::into))
    }

    fn write(&self, id: &str, material: &[u8]) -> anyhow::Result<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.data
            .lock()
            .unwrap()
            .insert(id.to_string(), material.to_vec());
        Ok(())
    }

    fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .keys()
            .filter(|id| id.starts_with(prefix))
            .cloned()
            .collect())
    }

    fn delete(&self, id: &str) -> anyhow::Result<bool> {
        Ok(self.data.lock().unwrap().remove(id).is_some())
    }
}

fn seeded_store() -> (CountingStore, ResourceBlob) {
    let store = CountingStore::default();
    let blob = mixed_resource_blob_for_test();
    save_resource_blob(&store, "resource", &blob).unwrap();
    store.reset_write_count();
    (store, blob)
}

fn assert_resource(store: &CountingStore, expected: &ResourceBlob) {
    assert_eq!(load_resource_blob(store, "resource").unwrap(), *expected);
}

#[test]
fn flat_writes_preserve_every_untouched_resource_section() {
    let (store, mut expected) = seeded_store();

    write_resource_entry(&store, "resource/env/NEW_TOKEN", b"new-env").unwrap();
    expected
        .env
        .insert("NEW_TOKEN".into(), SecretBytes::from("new-env"));
    assert_resource(&store, &expected);

    write_resource_entry(&store, "resource/env/PRIMARY_TOKEN", b"primary-v2").unwrap();
    expected
        .env
        .insert("PRIMARY_TOKEN".into(), SecretBytes::from("primary-v2"));
    assert_resource(&store, &expected);

    write_resource_entry(&store, "resource/files/config/new.toml", b"new = true\n").unwrap();
    expected
        .files
        .insert("config/new.toml".into(), SecretBytes::from("new = true\n"));
    assert_resource(&store, &expected);

    write_resource_entry(
        &store,
        "resource/files/config/primary.toml",
        b"primary = 2\n",
    )
    .unwrap();
    expected.files.insert(
        "config/primary.toml".into(),
        SecretBytes::from("primary = 2\n"),
    );
    assert_resource(&store, &expected);

    assert_eq!(store.write_count(), 4);
}

#[test]
fn flat_deletes_preserve_other_sections_and_skip_missing_save() {
    let (store, mut expected) = seeded_store();

    assert!(delete_resource_entry(&store, "resource/env/PRIMARY_TOKEN").unwrap());
    expected.env.remove("PRIMARY_TOKEN");
    assert_resource(&store, &expected);

    assert!(delete_resource_entry(&store, "resource/files/config/primary.toml").unwrap());
    expected.files.remove("config/primary.toml");
    assert_resource(&store, &expected);
    assert_eq!(store.write_count(), 2);

    let writes_before_missing = store.write_count();
    assert!(!delete_resource_entry(&store, "resource/env/MISSING_TOKEN").unwrap());
    assert_eq!(store.write_count(), writes_before_missing);
    assert_resource(&store, &expected);
}

#[test]
fn flat_internal_boundary_rejects_libsecret_without_saving() {
    let (store, expected) = seeded_store();

    let write_error = write_resource_entry(&store, "resource/libsecret/item", b"material")
        .expect_err("libsecret write must not enter the flat mutation path");
    assert!(write_error.to_string().contains("not a flat resource slot"));
    let delete_error = delete_resource_entry(&store, "resource/libsecret/item")
        .expect_err("libsecret delete must not enter the flat mutation path");
    assert!(
        delete_error
            .to_string()
            .contains("not a flat resource slot")
    );

    assert_eq!(store.write_count(), 0);
    assert_resource(&store, &expected);
}

#[test]
fn non_slotted_ids_keep_raw_storage_behavior() {
    let store = CountingStore::default();

    write_resource_entry(&store, "control/example", b"raw").unwrap();
    assert_eq!(
        read_resource_entry(&store, "control/example").unwrap(),
        Some(SecretBytes::from("raw"))
    );
    assert!(delete_resource_entry(&store, "control/example").unwrap());
    assert_eq!(store.read("control/example").unwrap(), None);
}
