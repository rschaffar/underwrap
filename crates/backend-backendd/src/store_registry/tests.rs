use super::*;
use std::path::Path;
use underwrap_protocol::config::{StoreRecord, StoreRegistryDocument};

fn write_registry(path: &Path, stores: Vec<StoreRecord>) {
    let doc = StoreRegistryDocument { stores };
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
}

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
fn missing_registry_file_is_tolerated() {
    let tmp = tempfile::TempDir::new().unwrap();
    let map = load_store_records(&tmp.path().join("stores.json")).unwrap();
    assert!(map.is_empty());
}

#[test]
fn registry_round_trips_by_store_id() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("stores.json");
    write_registry(
        &path,
        vec![plain_record(
            "store_work",
            "Work",
            &tmp.path().join("work-data"),
        )],
    );

    let map = load_store_records(&path).unwrap();
    assert_eq!(map.len(), 1);
    assert_eq!(map["store_work"].record.name, "Work");
}

#[test]
fn duplicate_visible_name_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("stores.json");
    write_registry(
        &path,
        vec![
            plain_record("store_a", "Shared", &tmp.path().join("a")),
            plain_record("store_b", "shared", &tmp.path().join("b")),
        ],
    );

    let err = load_store_records(&path).expect_err("duplicate visible name must abort");
    assert!(format!("{err:#}").contains("duplicate visible store name"));
}

#[test]
fn invalid_store_id_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("stores.json");
    write_registry(
        &path,
        vec![plain_record("Bad-Id", "Work", &tmp.path().join("x"))],
    );

    let err = load_store_records(&path).expect_err("invalid id must be refused");
    assert!(format!("{err:#}").contains("invalid store id"));
}

#[test]
fn description_metadata_round_trips() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("stores.json");
    let mut record = plain_record("store_work", "Work", &tmp.path().join("w"));
    record.description = Some("the work store".into());
    record.created_at = Some(123);
    write_registry(&path, vec![record]);
    let map = load_store_records(&path).unwrap();
    assert_eq!(
        map["store_work"].record.description.as_deref(),
        Some("the work store")
    );
    assert_eq!(map["store_work"].record.created_at, Some(123));
}
