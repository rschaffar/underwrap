use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use hyper::StatusCode;
use underwrap_protocol::service::{AdminStoreBackendKind, AdminStoreCreateRequest};

use super::*;
use crate::admin_api::stores::crud::{handle_admin_store_create, handle_admin_stores_list};
use crate::http::responses::Body;
use crate::runtime_state::ListenerState;
use crate::store_registry::StoreState;
use crate::test_support::{admin_principal, create_store_for_test, crud_state, run_handler};

fn is_locked(state: &ListenerState, store_id: &str) -> bool {
    matches!(
        state.shared.stores.read().unwrap().entry(store_id),
        Ok(entry) if matches!(entry.state, StoreState::Locked),
    )
}

fn encrypted_dir(state: &ListenerState, store_id: &str) -> std::path::PathBuf {
    let registry = state.shared.stores.read().unwrap();
    match &registry.entry(store_id).unwrap().record.storage {
        underwrap_protocol::config::StorageConfig::Encrypted { dir } => dir.clone(),
        underwrap_protocol::config::StorageConfig::Plain { .. } => {
            panic!("expected encrypted store")
        }
    }
}

struct HandlerHooks {
    failures: Vec<rotation::RotationPhase>,
}

impl rotation::RotationHooks for HandlerHooks {
    fn rotation_id(&self) -> String {
        "aaaaaaaaaaaaaaaa".into()
    }

    fn checkpoint(&self, phase: rotation::RotationPhase) -> std::io::Result<()> {
        if self.failures.contains(&phase) {
            Err(std::io::Error::other("injected"))
        } else {
            Ok(())
        }
    }
}

fn create_store(
    state: Arc<crate::runtime_state::ListenerState>,
    p: &crate::admin_api::csrf::Principal,
    backend: AdminStoreBackendKind,
    name: &str,
) -> serde_json::Value {
    let submitted_key = "11".repeat(32);
    let (status, mut body) = run_handler(handle_admin_store_create(
        State(state),
        Extension(p.clone()),
        Body(AdminStoreCreateRequest {
            name: name.into(),
            backend,
            description: None,
            key_hex: (backend == AdminStoreBackendKind::Encrypted)
                .then(|| underwrap_protocol::service::AdminKeyHex::new(submitted_key.clone())),
        }),
    ));
    assert_eq!(status, StatusCode::OK, "store create failed: {body}");
    if backend == AdminStoreBackendKind::Encrypted {
        body.as_object_mut()
            .unwrap()
            .insert("key_hex".into(), submitted_key.into());
    }
    body
}

#[tokio::test(flavor = "multi_thread")]
async fn rotate_key_reencrypts_resources_and_returns_new_value_once() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let create_body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = create_body["store"]["id"].as_str().unwrap().to_string();
    let original_key = create_body["key_hex"].as_str().unwrap().to_string();

    // Seed a known plaintext under the original key.
    {
        let backend = state
            .shared
            .stores
            .read()
            .unwrap()
            .expect_unlocked(&store_id);
        backend.write("greeting", b"hello").unwrap();
    }

    let (status, body) = run_handler(handle_admin_store_rotate_key(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(status, StatusCode::OK);
    let new_key = body["key_hex"].as_str().unwrap().to_string();
    assert_ne!(new_key, original_key, "rotation must produce a fresh key");
    assert_eq!(new_key.len(), 64);
    assert!(
        body["warning"]
            .as_str()
            .unwrap()
            .contains("re-encrypted under the new master key"),
        "warning must describe the re-encryption: {}",
        body["warning"],
    );

    // The active backend still decodes the same plaintext (now
    // under the new key).
    {
        let backend = state
            .shared
            .stores
            .read()
            .unwrap()
            .expect_unlocked(&store_id);
        assert_eq!(
            backend.read("greeting").unwrap().unwrap().as_ref(),
            b"hello"
        );
        // Canary survived the rotation.
        assert_eq!(
            backend
                .read(underwrap_backend_store::ENCRYPTED_CANARY_ID)
                .unwrap()
                .unwrap()
                .as_ref(),
            underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
        );
    }

    // Store-list payload still does not leak the key value.
    let (_, body) = run_handler(handle_admin_stores_list(State(state.clone())));
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(!body_str.contains(&new_key));
    assert!(!body_str.contains(&original_key));
}

#[tokio::test(flavor = "multi_thread")]
async fn rotation_does_not_write_the_unchanged_registry_record() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let created = create_store(
        state.clone(),
        &principal,
        AdminStoreBackendKind::Encrypted,
        "vault",
    );
    let store_id = created["store"]["id"].as_str().unwrap().to_string();
    let registry_path = state.admin().stores_dir.join(format!("{store_id}.json"));
    let before = std::fs::read(&registry_path).unwrap();
    let stores_dir = state.admin().stores_dir.clone();
    let displaced_registry = tmp.path().join("stores-registry-offline");
    std::fs::rename(&stores_dir, &displaced_registry).unwrap();
    std::fs::write(&stores_dir, b"registry writes must fail").unwrap();

    let (status, _) = run_handler(handle_admin_store_rotate_key(
        State(state.clone()),
        Extension(principal),
        Path(store_id.clone()),
    ));

    std::fs::remove_file(&stores_dir).unwrap();
    std::fs::rename(&displaced_registry, &stores_dir).unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(std::fs::read(&registry_path).unwrap(), before);
}

#[test]
fn unrecoverable_handler_failure_locks_store_and_returns_safe_stable_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let created = create_store(
        state.clone(),
        &principal,
        AdminStoreBackendKind::Encrypted,
        "vault",
    );
    let store_id = created["store"]["id"].as_str().unwrap().to_string();
    let old_key_hex = created["key_hex"].as_str().unwrap().to_string();
    let old_key = underwrap_backend_store::parse_hex_key(&old_key_hex).unwrap();
    let canonical = encrypted_dir(&state, &store_id);
    state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(&store_id)
        .write("resource/private-name", b"seeded-private-plaintext")
        .unwrap();
    let hooks = HandlerHooks {
        failures: vec![
            rotation::RotationPhase::PromoteStage,
            rotation::RotationPhase::RestoreBackup,
        ],
    };
    let (status, body) = run_handler(rotate_key_with_hooks(
        state.clone(),
        principal,
        store_id.clone(),
        &hooks,
    ));
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "rotation_recovery_required");
    let response_text = serde_json::to_string(&body).unwrap();
    assert!(!response_text.contains(&old_key_hex));
    assert!(!response_text.contains("private-name"));
    assert!(!response_text.contains("seeded-private-plaintext"));
    assert!(is_locked(&state, &store_id));
    assert!(!canonical.exists());
    let workspace = std::fs::read_dir(canonical.parent().unwrap())
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("underwrap-rotation")
        })
        .unwrap();
    let retained = underwrap_backend_store::EncryptedBackend::open_existing_with_key(
        &workspace.join("backup"),
        old_key,
    )
    .unwrap();
    assert_eq!(
        retained
            .read("resource/private-name")
            .unwrap()
            .unwrap()
            .as_ref(),
        b"seeded-private-plaintext"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rotate_key_refuses_plain_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
            key_hex: None,
        },
    );
    let (status, body) = run_handler(handle_admin_store_rotate_key(
        State(state.clone()),
        Extension(p),
        Path(store_id),
    ));
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "not_encrypted");
}

#[tokio::test(flavor = "multi_thread")]
async fn rotate_key_refuses_locked_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
            key_hex: Some(underwrap_protocol::service::AdminKeyHex::new(
                "11".repeat(32),
            )),
        },
    );
    // Lock the store and confirm rotation refuses.
    let (lock_status, _) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(lock_status, StatusCode::OK);
    let (status, body) = run_handler(handle_admin_store_rotate_key(
        State(state.clone()),
        Extension(p),
        Path(store_id),
    ));
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "store_locked");
}

#[test]
fn unlock_round_trip_succeeds_with_correct_hex() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let key_hex = body["key_hex"].as_str().unwrap().to_string();

    // Simulate a restart by locking the store.
    let (lock_status, _) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(lock_status, StatusCode::OK);
    assert!(is_locked(&state, &store_id));

    let (status, _) = run_handler(handle_admin_store_unlock(
        State(state.clone()),
        Extension(p),
        Path(store_id.clone()),
        Body(underwrap_protocol::service::AdminStoreUnlockRequest { key_hex }),
    ));
    assert_eq!(status, StatusCode::OK);
    assert!(!is_locked(&state, &store_id));
}

#[test]
fn unlock_missing_canonical_requires_host_recovery_without_recreating_it() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let created = create_store(
        state.clone(),
        &principal,
        AdminStoreBackendKind::Encrypted,
        "vault",
    );
    let store_id = created["store"]["id"].as_str().unwrap().to_string();
    let key_hex = created["key_hex"].as_str().unwrap().to_string();
    let canonical = encrypted_dir(&state, &store_id);
    let retained = canonical.with_extension("retained-test");
    let (lock_status, _) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(principal.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(lock_status, StatusCode::OK);
    std::fs::rename(&canonical, &retained).unwrap();

    let (status, body) = run_handler(handle_admin_store_unlock(
        State(state.clone()),
        Extension(principal),
        Path(store_id.clone()),
        Body(underwrap_protocol::service::AdminStoreUnlockRequest { key_hex }),
    ));
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "rotation_recovery_required");
    assert!(is_locked(&state, &store_id));
    assert!(!canonical.exists());
    assert!(retained.is_dir());
}

#[test]
fn unlock_rejects_wrong_hex_and_keeps_store_locked() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let (lock_status, _) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(lock_status, StatusCode::OK);

    let bogus_hex = "0".repeat(64);
    let (status, resp_body) = run_handler(handle_admin_store_unlock(
        State(state.clone()),
        Extension(p),
        Path(store_id.clone()),
        Body(underwrap_protocol::service::AdminStoreUnlockRequest { key_hex: bogus_hex }),
    ));
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(resp_body["code"], "key_mismatch");
    assert!(is_locked(&state, &store_id));
}

#[test]
fn unlock_rejects_invalid_hex_format() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let (lock_status, _) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(lock_status, StatusCode::OK);
    let (status, resp_body) = run_handler(handle_admin_store_unlock(
        State(state.clone()),
        Extension(p),
        Path(store_id),
        Body(underwrap_protocol::service::AdminStoreUnlockRequest {
            key_hex: "not hex".into(),
        }),
    ));
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(resp_body["code"], "invalid_key_hex");
}

#[test]
fn unlock_refuses_plain_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
            key_hex: None,
        },
    );
    let (status, body) = run_handler(handle_admin_store_unlock(
        State(state.clone()),
        Extension(p),
        Path(store_id),
        Body(underwrap_protocol::service::AdminStoreUnlockRequest {
            key_hex: "0".repeat(64),
        }),
    ));
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "store_not_encrypted");
}

#[test]
fn unlock_refuses_already_unlocked_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let key_hex = body["key_hex"].as_str().unwrap().to_string();
    let (status, resp_body) = run_handler(handle_admin_store_unlock(
        State(state.clone()),
        Extension(p),
        Path(store_id),
        Body(underwrap_protocol::service::AdminStoreUnlockRequest { key_hex }),
    ));
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(resp_body["code"], "already_unlocked");
}

#[test]
fn lock_transitions_unlocked_encrypted_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let body = create_store(state.clone(), &p, AdminStoreBackendKind::Encrypted, "vault");
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    assert!(!is_locked(&state, &store_id));
    let (status, body) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p.clone()),
        Path(store_id.clone()),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["was_locked"], false);
    assert!(is_locked(&state, &store_id));
    // Second call is a no-op success.
    let (status, body) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p),
        Path(store_id),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["was_locked"], true);
}

#[test]
fn lock_refuses_plain_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let p = admin_principal();
    let store_id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
            key_hex: None,
        },
    );
    let (status, body) = run_handler(handle_admin_store_lock(
        State(state.clone()),
        Extension(p),
        Path(store_id),
    ));
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "store_not_encrypted");
}
