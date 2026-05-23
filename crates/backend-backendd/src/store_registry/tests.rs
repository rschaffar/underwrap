use super::*;
use std::path::Path;
use underwrap_protocol::config::StoreRecord;

fn plain_record(id: &str, name: &str, dir: &Path) -> StoreRecord {
    StoreRecord {
        id: id.to_string(),
        name: name.to_string(),
        description: None,
        created_at: None,
        storage: underwrap_protocol::config::StorageConfig::Plain {
            dir: dir.to_path_buf(),
        },
    }
}

#[test]
fn missing_stores_dir_is_tolerated() {
    let tmp = tempfile::TempDir::new().unwrap();
    let map = load_store_records(&tmp.path().join("stores")).unwrap();
    assert!(map.is_empty());
}

#[test]
fn registry_round_trips_by_store_id() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("stores");
    let record = plain_record("store_work", "Work", &tmp.path().join("work-data"));
    save_store_record(&dir, &record).unwrap();

    let map = load_store_records(&dir).unwrap();
    assert_eq!(map.len(), 1);
    assert_eq!(map["store_work"].record.name, "Work");
    assert_eq!(map["store_work"].record.id, "store_work");
}

#[test]
fn id_lives_in_filename_not_body() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("stores");
    let record = plain_record("store_work", "Work", &tmp.path().join("w"));
    save_store_record(&dir, &record).unwrap();
    let text = std::fs::read_to_string(dir.join("store_work.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    let obj = parsed.as_object().unwrap();
    assert!(
        !obj.contains_key("id"),
        "on-disk store JSON must not contain an 'id' key: {obj:?}",
    );
}

#[test]
fn duplicate_visible_name_across_files_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("stores");
    save_store_record(
        &dir,
        &plain_record("store_a", "Shared", &tmp.path().join("a")),
    )
    .unwrap();
    save_store_record(
        &dir,
        &plain_record("store_b", "shared", &tmp.path().join("b")),
    )
    .unwrap();

    let err = load_store_records(&dir).expect_err("duplicate visible name must abort");
    assert!(format!("{err:#}").contains("duplicate visible store name"));
}

#[test]
fn invalid_store_id_in_filename_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("stores");
    std::fs::create_dir_all(&dir).unwrap();
    // `save_store_record` would refuse the bad id; write the file by
    // hand to simulate filesystem tampering and assert the loader is
    // the one to catch it.
    let json =
        serde_json::to_vec_pretty(&plain_record("placeholder", "Work", &tmp.path().join("x")))
            .unwrap();
    std::fs::write(dir.join("Bad-Id.json"), &json).unwrap();

    let err = load_store_records(&dir).expect_err("invalid id must be refused");
    assert!(format!("{err:#}").contains("invalid store id"));
}

#[test]
fn description_metadata_round_trips() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("stores");
    let mut record = plain_record("store_work", "Work", &tmp.path().join("w"));
    record.description = Some("the work store".into());
    record.created_at = Some(123);
    save_store_record(&dir, &record).unwrap();
    let map = load_store_records(&dir).unwrap();
    assert_eq!(
        map["store_work"].record.description.as_deref(),
        Some("the work store")
    );
    assert_eq!(map["store_work"].record.created_at, Some(123));
}

#[test]
fn delete_store_record_removes_file_and_is_idempotent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("stores");
    let record = plain_record("store_gone", "Gone", &tmp.path().join("g"));
    save_store_record(&dir, &record).unwrap();
    assert!(dir.join("store_gone.json").exists());
    delete_store_record(&dir, "store_gone").unwrap();
    assert!(!dir.join("store_gone.json").exists());
    // Second delete is a no-op (idempotent under retry).
    delete_store_record(&dir, "store_gone").unwrap();
}

#[test]
fn non_json_files_are_ignored() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("stores");
    save_store_record(&dir, &plain_record("store_x", "X", &tmp.path().join("x"))).unwrap();
    std::fs::write(dir.join("store_x.json.bak"), b"junk").unwrap();
    std::fs::write(dir.join("README"), b"notes").unwrap();
    let map = load_store_records(&dir).unwrap();
    assert_eq!(map.len(), 1);
    assert!(map.contains_key("store_x"));
}
