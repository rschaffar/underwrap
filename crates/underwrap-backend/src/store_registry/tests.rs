use super::*;
use std::path::Path;
use underwrap_backend_store::{EncryptedBackend, StorageBackend};
use underwrap_protocol::config::StoreRecord;

use crate::test_support::tree_snapshot;

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

fn encrypted_record(id: &str, dir: &Path) -> StoreRecord {
    StoreRecord {
        id: id.to_string(),
        name: "Encrypted".into(),
        description: None,
        created_at: None,
        storage: underwrap_protocol::config::StorageConfig::Encrypted {
            dir: dir.to_path_buf(),
        },
    }
}

fn create_generation(path: &Path, key: [u8; 32], marker: &[u8], include_canary: bool) {
    let backend = EncryptedBackend::create_with_key(path, key).unwrap();
    if include_canary {
        backend
            .write(
                underwrap_backend_store::ENCRYPTED_CANARY_ID,
                underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
            )
            .unwrap();
    }
    backend.write("resource/marker", marker).unwrap();
}

fn create_private_directory(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }
    #[cfg(not(unix))]
    std::fs::create_dir(path).unwrap();
}

#[test]
fn startup_preserves_every_process_visible_rotation_shape_without_reconciliation() {
    struct Shape {
        name: &'static str,
        canonical_key: Option<[u8; 32]>,
        backup: bool,
        stage: bool,
    }
    for shape in [
        Shape {
            name: "canonical-old-plus-incomplete-stage",
            canonical_key: Some([3; 32]),
            backup: false,
            stage: true,
        },
        Shape {
            name: "missing-plus-backup-plus-stage",
            canonical_key: None,
            backup: true,
            stage: true,
        },
        Shape {
            name: "canonical-new-plus-backup",
            canonical_key: Some([9; 32]),
            backup: true,
            stage: false,
        },
        Shape {
            name: "missing-plus-backup",
            canonical_key: None,
            backup: true,
            stage: false,
        },
    ] {
        let tmp = tempfile::TempDir::new().unwrap();
        let canonical = tmp.path().join("store_e");
        if let Some(key) = shape.canonical_key {
            create_generation(&canonical, key, shape.name.as_bytes(), true);
        }
        let workspace = tmp
            .path()
            .join(".store_e.underwrap-rotation-1111111111111111");
        create_private_directory(&workspace);
        if shape.backup {
            create_generation(&workspace.join("backup"), [3; 32], b"old", true);
        }
        if shape.stage {
            create_generation(&workspace.join("stage"), [9; 32], b"new-incomplete", false);
        }

        let before = tree_snapshot(tmp.path());
        let entries = open_initial_stores(HashMap::from([(
            "store_e".into(),
            encrypted_record("store_e", &canonical),
        )]))
        .unwrap();
        assert!(matches!(entries["store_e"].state, StoreState::Locked));
        assert_eq!(
            tree_snapshot(tmp.path()),
            before,
            "shape {} changed",
            shape.name
        );

        match shape.canonical_key {
            Some(key) => {
                let backend = EncryptedBackend::open_existing_with_key(&canonical, key).unwrap();
                assert_eq!(
                    backend.read("resource/marker").unwrap().unwrap().as_ref(),
                    shape.name.as_bytes()
                );
            }
            None => {
                assert!(EncryptedBackend::open_existing_with_key(&canonical, [3; 32]).is_err());
                assert!(
                    !canonical.exists(),
                    "shape {} recreated canonical",
                    shape.name
                );
            }
        }
        assert_eq!(
            tree_snapshot(tmp.path()),
            before,
            "shape {} changed after open",
            shape.name
        );
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
    assert_eq!(map["store_work"].name, "Work");
    assert_eq!(map["store_work"].id, "store_work");
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
    assert!(format!("{err:#}").contains("duplicate store visible name"));
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
        map["store_work"].description.as_deref(),
        Some("the work store")
    );
    assert_eq!(map["store_work"].created_at, Some(123));
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

#[tokio::test]
async fn migration_lease_counts_complete_scopes_and_notifies_drain() {
    let tmp = tempfile::TempDir::new().unwrap();
    let backend: Arc<dyn StorageBackend> =
        Arc::new(underwrap_backend_store::PlainBackend::create(&tmp.path().join("data")).unwrap());
    let entry = StoreEntry::new(
        plain_record("store_x", "X", &tmp.path().join("data")),
        StoreState::Ready(backend),
    );
    let tracker = Arc::clone(&entry.leases);
    let registry = StoreRegistry {
        entries: HashMap::from([("store_x".into(), entry)]),
    };
    let first = registry.lease("store_x").unwrap();
    let second = registry.lease("store_x").unwrap();
    assert_eq!(tracker.count(), 2);
    let waiter = tokio::spawn({
        let tracker = Arc::clone(&tracker);
        async move { tracker.wait_for_zero(Duration::from_secs(1)).await }
    });
    tokio::task::yield_now().await;
    drop(first);
    assert_eq!(tracker.count(), 1);
    drop(second);
    assert!(waiter.await.unwrap());
    assert_eq!(tracker.count(), 0);
}

#[test]
fn migration_state_rejects_busy_and_recovery_required_leases() {
    let tmp = tempfile::TempDir::new().unwrap();
    for (state, expected) in [
        (StoreState::Busy, RegistryLookupError::Busy),
        (
            StoreState::RecoveryRequired,
            RegistryLookupError::RecoveryRequired,
        ),
        (StoreState::Locked, RegistryLookupError::Locked),
    ] {
        let registry = StoreRegistry {
            entries: HashMap::from([(
                "store_x".into(),
                StoreEntry::new(plain_record("store_x", "X", tmp.path()), state),
            )]),
        };
        assert!(matches!(registry.lease("store_x"), Err(error) if error == expected));
    }
}

#[test]
fn migration_workspace_detection_is_exact_and_storage_neutral() {
    fn assert_recovery(tmp: &tempfile::TempDir, record: StoreRecord) {
        let before = tree_snapshot(tmp.path());
        let entries = open_initial_stores(HashMap::from([("store_x".into(), record)])).unwrap();
        assert!(matches!(
            entries["store_x"].state,
            StoreState::RecoveryRequired
        ));
        assert_eq!(tree_snapshot(tmp.path()), before);
    }

    for layout in ["pre-swap", "backup", "promoted", "malformed", "multiple"] {
        let tmp = tempfile::TempDir::new().unwrap();
        let canonical = tmp.path().join("store_x");
        if layout != "backup" {
            underwrap_backend_store::PlainBackend::create(&canonical).unwrap();
            std::fs::write(canonical.join("marker"), b"canonical").unwrap();
        }
        let workspace = tmp
            .path()
            .join(".store_x.underwrap-migration-1111111111111111");
        create_private_directory(&workspace);
        match layout {
            "pre-swap" => create_private_directory(&workspace.join("stage")),
            "backup" => create_private_directory(&workspace.join("backup")),
            "promoted" => create_private_directory(&workspace.join("backup")),
            "malformed" => {
                std::fs::write(workspace.join("manifest.json"), b"malformed").unwrap();
            }
            "multiple" => create_private_directory(
                &tmp.path()
                    .join(".store_x.underwrap-migration-abcdef0123456789"),
            ),
            _ => unreachable!(),
        }
        assert_recovery(&tmp, plain_record("store_x", "X", &canonical));
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let canonical = tmp.path().join("store_x");
    underwrap_backend_store::PlainBackend::create(&canonical).unwrap();
    create_private_directory(
        &tmp.path()
            .join(".store_x.underwrap-migration-2222222222222222"),
    );
    assert_recovery(&tmp, encrypted_record("store_x", &canonical));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::TempDir::new().unwrap();
        let canonical = tmp.path().join("store_x");
        underwrap_backend_store::PlainBackend::create(&canonical).unwrap();
        let residue = tmp.path().join("residue");
        create_private_directory(&residue);
        symlink(
            &residue,
            tmp.path()
                .join(".store_x.underwrap-migration-3333333333333333"),
        )
        .unwrap();
        assert_recovery(&tmp, plain_record("store_x", "X", &canonical));
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let canonical = tmp.path().join("store_x");
    underwrap_backend_store::PlainBackend::create(&canonical).unwrap();
    create_private_directory(
        &tmp.path()
            .join(".store_x.underwrap-migration-ABCDEF0123456789"),
    );
    let entries = open_initial_stores(HashMap::from([(
        "store_x".into(),
        plain_record("store_x", "X", &canonical),
    )]))
    .unwrap();
    assert!(matches!(entries["store_x"].state, StoreState::Ready(_)));
}
