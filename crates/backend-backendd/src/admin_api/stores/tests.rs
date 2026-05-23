use std::collections::HashMap;
use std::sync::Arc;

use underwrap_store::StorageBackend;

use super::*;
use crate::test_support::{MemTestStore, crud_state, expect_404, test_admin_state};

#[test]
fn admin_store_resolves_existing_store() {
    let store: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
    let state = test_admin_state(HashMap::from([("work".to_string(), store)]));
    assert!(admin_store(&state, "work").is_ok());
}

#[test]
fn admin_store_unknown_store_returns_404() {
    let state = test_admin_state(HashMap::new());
    expect_404(admin_store(&state, "missing"));
}

#[test]
fn resolve_store_dir_uses_id_as_leaf_under_root() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let resolved =
        resolve_store_dir(&state, "store_deadbeef").expect("must accept when root is set");
    assert_eq!(resolved, tmp.path().join("data").join("store_deadbeef"));
}

#[test]
fn resolve_store_dir_rejects_when_root_unconfigured() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut state = crud_state(tmp.path());
    state.stores_data_root = None;
    let err = resolve_store_dir(&state, "store_x").expect_err("must refuse without root");
    assert_eq!(err.0, "data_root_not_configured");
}

#[test]
fn resolve_store_dir_rejects_relative_root() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut state = crud_state(tmp.path());
    state.stores_data_root = Some(std::path::PathBuf::from("relative/data"));
    let err = resolve_store_dir(&state, "store_x").expect_err("must refuse relative root");
    assert_eq!(err.0, "data_root_not_absolute");
}
