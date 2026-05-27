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

#[test]
fn dump_with_cap_accepts_state_under_the_limit() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let path = tempdir.path().join("libsecret-state.jcs");

    // A generous cap admits a normal keyring.
    dump_with_cap(&populated_store(), &path, 64 * 1024).unwrap();
    let read = std::fs::read(&path).unwrap();
    let _ = LibsecretSection::from_canonical_bytes(&read).unwrap();
}

#[test]
fn dump_with_cap_rejects_oversized_state_and_preserves_prior_file() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let path = tempdir.path().join("libsecret-state.jcs");

    // Persist a valid baseline within a generous cap.
    dump_with_cap(&populated_store(), &path, 64 * 1024).unwrap();
    let baseline = std::fs::read(&path).unwrap();

    // A subsequent mutation whose canonical form exceeds a tiny cap must be
    // rejected, and the previously-persisted state must remain intact (the
    // cap is checked before the on-disk file is touched).
    let err = dump_with_cap(&populated_store(), &path, 4).unwrap_err();
    assert!(
        format!("{err:#}").contains("exceeding"),
        "unexpected error: {err:#}"
    );

    let after = std::fs::read(&path).unwrap();
    assert_eq!(
        after, baseline,
        "rejected write must not alter the state file"
    );
    // And no stray temp file is left behind.
    let tmp = tempdir.path().join(".libsecret-state.jcs.new");
    assert!(
        !tmp.exists(),
        "temp file should not be left after a rejected write"
    );
}

#[test]
fn default_cap_is_generous_relative_to_a_normal_keyring() {
    // Sanity: the built-in default comfortably admits a populated keyring, so
    // the cap only bites pathological growth, not real usage.
    let section_len = crate::section_store::section_from_store(&populated_store())
        .to_canonical_bytes()
        .unwrap()
        .len() as u64;
    assert!(section_len < DEFAULT_MAX_STATE_BYTES);
}
