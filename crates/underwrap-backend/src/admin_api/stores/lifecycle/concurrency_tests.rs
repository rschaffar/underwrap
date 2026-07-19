use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use axum::Extension;
use axum::extract::{Path as AxumPath, State};
use axum::response::IntoResponse;
use hyper::StatusCode;
use underwrap_backend_store::{EncryptedBackend, StorageBackend};
use underwrap_protocol::service::{AdminStoreBackendKind, AdminStoreCreateRequest};

use super::*;
use crate::admin_api::stores::crud::handle_admin_store_create;
use crate::http::responses::Body;
use crate::store_registry::{StoreEntry, StoreState};
use crate::test_support::{
    MemTestStore, admin_principal, assert_secure_encrypted_tree, crud_state, run_handler,
    tree_snapshot,
};

fn create_encrypted_store(
    state: Arc<ListenerState>,
    principal: &Principal,
) -> (String, String, PathBuf) {
    let (status, body) = run_handler(handle_admin_store_create(
        State(state.clone()),
        Extension(principal.clone()),
        Body(AdminStoreCreateRequest {
            name: "vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        }),
    ));
    assert_eq!(status, StatusCode::OK, "store create failed: {body}");
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let key_hex = body["key_hex"].as_str().unwrap().to_string();
    let canonical = match &state
        .shared
        .stores
        .read()
        .unwrap()
        .entry(&store_id)
        .unwrap()
        .record
        .storage
    {
        underwrap_protocol::config::StorageConfig::Encrypted { dir } => dir.clone(),
        underwrap_protocol::config::StorageConfig::Plain { .. } => unreachable!(),
    };
    state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(&store_id)
        .write("resource/private-path", b"rotation-private-value")
        .unwrap();
    (store_id, key_hex, canonical)
}

fn workspace(canonical: &Path, id: &str) -> PathBuf {
    canonical.parent().unwrap().join(format!(
        ".{}.underwrap-rotation-{id}",
        canonical.file_name().unwrap().to_string_lossy()
    ))
}

fn assert_generation(path: &Path, key: [u8; 32]) {
    let backend = EncryptedBackend::open_existing_with_key(path, key).unwrap();
    assert_eq!(
        backend
            .read(underwrap_backend_store::ENCRYPTED_CANARY_ID)
            .unwrap()
            .unwrap()
            .as_ref(),
        underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT
    );
    assert_eq!(
        backend
            .read("resource/private-path")
            .unwrap()
            .unwrap()
            .as_ref(),
        b"rotation-private-value"
    );
}

async fn error_body(error: AdminError) -> serde_json::Value {
    let response = error.into_response();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

struct FixedHooks {
    id: &'static str,
    key: [u8; 32],
    failures: HashSet<rotation::RotationPhase>,
    before_rotation_lock: Option<Arc<dyn Fn() + Send + Sync>>,
    before_publish: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl FixedHooks {
    fn new(id: &'static str, key: [u8; 32]) -> Self {
        Self {
            id,
            key,
            failures: HashSet::new(),
            before_rotation_lock: None,
            before_publish: None,
        }
    }
}

impl rotation::RotationHooks for FixedHooks {
    fn rotation_id(&self) -> String {
        self.id.into()
    }

    fn replacement_key(&self) -> [u8; 32] {
        self.key
    }

    fn checkpoint(&self, phase: rotation::RotationPhase) -> std::io::Result<()> {
        if self.failures.contains(&phase) {
            Err(std::io::Error::other("injected"))
        } else {
            Ok(())
        }
    }

    fn before_rotation_lock(&self) {
        if let Some(callback) = &self.before_rotation_lock {
            callback();
        }
    }

    fn before_publish(&self) {
        if let Some(callback) = &self.before_publish {
            callback();
        }
    }
}

#[derive(Default)]
struct GateState {
    entered: bool,
    released: bool,
}

struct BlockingHooks {
    id: &'static str,
    key: [u8; 32],
    phase: rotation::RotationPhase,
    gate: Arc<(Mutex<GateState>, Condvar)>,
}

impl BlockingHooks {
    fn wait_until_blocked(&self) {
        let (lock, cv) = &*self.gate;
        let state = lock.lock().unwrap();
        let (state, timeout) = cv
            .wait_timeout_while(state, Duration::from_secs(10), |state| !state.entered)
            .unwrap();
        assert!(!timeout.timed_out(), "rotation did not reach blocking hook");
        assert!(state.entered);
    }

    fn release(&self) {
        let (lock, cv) = &*self.gate;
        lock.lock().unwrap().released = true;
        cv.notify_all();
    }
}

impl rotation::RotationHooks for BlockingHooks {
    fn rotation_id(&self) -> String {
        self.id.into()
    }

    fn replacement_key(&self) -> [u8; 32] {
        self.key
    }

    fn checkpoint(&self, phase: rotation::RotationPhase) -> std::io::Result<()> {
        if phase == self.phase {
            let (lock, cv) = &*self.gate;
            let mut state = lock.lock().unwrap();
            state.entered = true;
            cv.notify_all();
            while !state.released {
                state = cv.wait(state).unwrap();
            }
        }
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_actual_overlapping_handlers_use_successive_admitted_generations() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let (store_id, old_key_hex, canonical) = create_encrypted_store(state.clone(), &principal);
    let old_key = underwrap_backend_store::parse_hex_key(&old_key_hex).unwrap();
    let first_hooks = Arc::new(BlockingHooks {
        id: "1111111111111111",
        key: [10; 32],
        phase: rotation::RotationPhase::ListSource,
        gate: Arc::default(),
    });
    let first_task = {
        let state = state.clone();
        let principal = principal.clone();
        let store_id = store_id.clone();
        let hooks = first_hooks.clone();
        tokio::spawn(async move {
            rotate_key_with_hooks(state, principal, store_id, hooks.as_ref()).await
        })
    };
    first_hooks.wait_until_blocked();

    let second_lock_attempt = Arc::new((Mutex::new(false), Condvar::new()));
    let second_task = {
        let state = state.clone();
        let principal = principal.clone();
        let store_id = store_id.clone();
        let lock_attempt = second_lock_attempt.clone();
        tokio::spawn(async move {
            let mut hooks = FixedHooks::new("2222222222222222", [11; 32]);
            hooks.before_rotation_lock = Some(Arc::new(move || {
                let (lock, cv) = &*lock_attempt;
                *lock.lock().unwrap() = true;
                cv.notify_all();
            }));
            rotate_key_with_hooks(state, principal, store_id, &hooks).await
        })
    };
    {
        let (lock, cv) = &*second_lock_attempt;
        let attempted = lock.lock().unwrap();
        let (attempted, timeout) = cv
            .wait_timeout_while(attempted, Duration::from_secs(10), |attempted| !*attempted)
            .unwrap();
        assert!(
            !timeout.timed_out(),
            "second handler did not attempt the rotation lock"
        );
        assert!(*attempted);
    }
    first_hooks.release();

    let first = first_task.await.unwrap().unwrap().0;
    let second = second_task.await.unwrap().unwrap().0;
    assert_eq!(first.key_hex, hex::encode([10; 32]));
    assert_eq!(second.key_hex, hex::encode([11; 32]));
    assert_generation(
        &workspace(&canonical, "1111111111111111").join("backup"),
        old_key,
    );
    assert_generation(
        &workspace(&canonical, "2222222222222222").join("backup"),
        [10; 32],
    );
    assert_generation(&canonical, [11; 32]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_lock_is_not_overwritten_by_late_rotation_publication() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let (store_id, old_key_hex, canonical) = create_encrypted_store(state.clone(), &principal);
    let old_key = underwrap_backend_store::parse_hex_key(&old_key_hex).unwrap();
    let hooks = Arc::new(BlockingHooks {
        id: "1111111111111111",
        key: [10; 32],
        phase: rotation::RotationPhase::VerifyPromoted,
        gate: Arc::default(),
    });
    let rotate_task = {
        let state = state.clone();
        let principal = principal.clone();
        let store_id = store_id.clone();
        let hooks = hooks.clone();
        tokio::spawn(async move {
            rotate_key_with_hooks(state, principal, store_id, hooks.as_ref()).await
        })
    };
    hooks.wait_until_blocked();
    let _ = handle_admin_store_lock(
        State(state.clone()),
        Extension(principal),
        AxumPath(store_id.clone()),
    )
    .await
    .unwrap();
    hooks.release();

    let body = error_body(rotate_task.await.unwrap().unwrap_err()).await;
    assert_eq!(body["code"], "rotation_recovery_required");
    assert!(matches!(
        state
            .shared
            .stores
            .read()
            .unwrap()
            .entry(&store_id)
            .unwrap()
            .state,
        StoreState::Locked
    ));
    assert_generation(&canonical, [10; 32]);
    assert_generation(
        &workspace(&canonical, "1111111111111111").join("backup"),
        old_key,
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_entry_at_publication_stays_missing_and_returns_recovery_required() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let (store_id, _, canonical) = create_encrypted_store(state.clone(), &principal);
    let callback_state = state.clone();
    let callback_id = store_id.clone();
    let mut hooks = FixedHooks::new("1111111111111111", [10; 32]);
    hooks.before_publish = Some(Arc::new(move || {
        callback_state
            .shared
            .stores
            .write()
            .unwrap()
            .entries
            .remove(&callback_id);
    }));

    let body = error_body(
        rotate_key_with_hooks(state.clone(), principal, store_id.clone(), &hooks)
            .await
            .unwrap_err(),
    )
    .await;
    assert_eq!(body["code"], "rotation_recovery_required");
    assert!(
        !state
            .shared
            .stores
            .read()
            .unwrap()
            .entries
            .contains_key(&store_id)
    );
    assert_generation(&canonical, [10; 32]);
}

#[tokio::test(flavor = "multi_thread")]
async fn replacement_entry_at_publication_is_left_untouched() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let (store_id, _, canonical) = create_encrypted_store(state.clone(), &principal);
    let replacement_path = tmp.path().join("replacement-store");
    let replacement_backend =
        Arc::new(EncryptedBackend::create_with_key(&replacement_path, [77; 32]).unwrap());
    replacement_backend
        .write(
            underwrap_backend_store::ENCRYPTED_CANARY_ID,
            underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
        )
        .unwrap();
    replacement_backend
        .write("replacement-marker", b"replacement")
        .unwrap();
    let replacement_backend_dyn: Arc<dyn StorageBackend> = replacement_backend.clone();
    let replacement_record = {
        let registry = state.shared.stores.read().unwrap();
        let mut record = registry.entry(&store_id).unwrap().record.clone();
        record.storage = underwrap_protocol::config::StorageConfig::Encrypted {
            dir: replacement_path.clone(),
        };
        record
    };
    let callback_state = state.clone();
    let callback_id = store_id.clone();
    let callback_backend = replacement_backend_dyn.clone();
    let mut hooks = FixedHooks::new("1111111111111111", [10; 32]);
    hooks.before_publish = Some(Arc::new(move || {
        callback_state
            .shared
            .stores
            .write()
            .unwrap()
            .entries
            .insert(
                callback_id.clone(),
                StoreEntry::new(
                    replacement_record.clone(),
                    StoreState::Unlocked(callback_backend.clone()),
                ),
            );
    }));

    let body = error_body(
        rotate_key_with_hooks(state.clone(), principal, store_id.clone(), &hooks)
            .await
            .unwrap_err(),
    )
    .await;
    assert_eq!(body["code"], "rotation_recovery_required");
    let registry = state.shared.stores.read().unwrap();
    let entry = registry.entry(&store_id).unwrap();
    assert!(matches!(
        &entry.state,
        StoreState::Unlocked(backend) if Arc::ptr_eq(backend, &replacement_backend_dyn)
    ));
    assert!(matches!(
        &entry.record.storage,
        underwrap_protocol::config::StorageConfig::Encrypted { dir } if dir == &replacement_path
    ));
    drop(registry);
    assert_eq!(
        replacement_backend
            .read("replacement-marker")
            .unwrap()
            .unwrap()
            .as_ref(),
        b"replacement"
    );
    assert_generation(&canonical, [10; 32]);
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_same_entry_backend_at_publication_is_left_untouched() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let (store_id, _, canonical) = create_encrypted_store(state.clone(), &principal);
    let replacement: Arc<dyn StorageBackend> = Arc::new(MemTestStore::new());
    replacement
        .write("replacement-marker", b"replacement")
        .unwrap();
    let callback_state = state.clone();
    let callback_id = store_id.clone();
    let callback_backend = replacement.clone();
    let mut hooks = FixedHooks::new("1111111111111111", [10; 32]);
    hooks.before_publish = Some(Arc::new(move || {
        callback_state
            .shared
            .stores
            .write()
            .unwrap()
            .entries
            .get_mut(&callback_id)
            .unwrap()
            .state = StoreState::Unlocked(callback_backend.clone());
    }));

    let body = error_body(
        rotate_key_with_hooks(state.clone(), principal, store_id.clone(), &hooks)
            .await
            .unwrap_err(),
    )
    .await;
    assert_eq!(body["code"], "rotation_recovery_required");
    assert!(matches!(
        &state.shared.stores.read().unwrap().entry(&store_id).unwrap().state,
        StoreState::Unlocked(backend) if Arc::ptr_eq(backend, &replacement)
    ));
    assert_eq!(
        replacement
            .read("replacement-marker")
            .unwrap()
            .unwrap()
            .as_ref(),
        b"replacement"
    );
    assert_generation(&canonical, [10; 32]);
}

struct ReplaceOnRestoreFailure {
    state: Arc<ListenerState>,
    store_id: String,
    replacement: Mutex<Option<StoreEntry>>,
}

impl rotation::RotationHooks for ReplaceOnRestoreFailure {
    fn rotation_id(&self) -> String {
        "1111111111111111".into()
    }

    fn replacement_key(&self) -> [u8; 32] {
        [10; 32]
    }

    fn checkpoint(&self, phase: rotation::RotationPhase) -> std::io::Result<()> {
        if phase == rotation::RotationPhase::OpenPromoted {
            return Err(std::io::Error::other("force rollback"));
        }
        if phase == rotation::RotationPhase::RestoreBackup {
            let replacement = self.replacement.lock().unwrap().take().unwrap();
            self.state
                .shared
                .stores
                .write()
                .unwrap()
                .entries
                .insert(self.store_id.clone(), replacement);
            return Err(std::io::Error::other("force rollback failure"));
        }
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn lock_disposition_does_not_lock_a_replacement_entry() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let (store_id, _, _) = create_encrypted_store(state.clone(), &principal);
    let replacement_path = tmp.path().join("replacement-after-failure");
    let replacement: Arc<dyn StorageBackend> =
        Arc::new(EncryptedBackend::create_with_key(&replacement_path, [77; 32]).unwrap());
    replacement
        .write(
            underwrap_backend_store::ENCRYPTED_CANARY_ID,
            underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
        )
        .unwrap();
    let replacement_record = {
        let registry = state.shared.stores.read().unwrap();
        let mut record = registry.entry(&store_id).unwrap().record.clone();
        record.storage = underwrap_protocol::config::StorageConfig::Encrypted {
            dir: replacement_path,
        };
        record
    };
    let hooks = ReplaceOnRestoreFailure {
        state: state.clone(),
        store_id: store_id.clone(),
        replacement: Mutex::new(Some(StoreEntry::new(
            replacement_record,
            StoreState::Unlocked(replacement.clone()),
        ))),
    };

    let body = error_body(
        rotate_key_with_hooks(state.clone(), principal, store_id.clone(), &hooks)
            .await
            .unwrap_err(),
    )
    .await;
    assert_eq!(body["code"], "rotation_recovery_required");
    assert!(matches!(
        &state.shared.stores.read().unwrap().entry(&store_id).unwrap().state,
        StoreState::Unlocked(backend) if Arc::ptr_eq(backend, &replacement)
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn three_handler_rotations_preserve_raw_trees_when_middle_response_is_dropped() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let (store_id, old_key_hex, canonical) = create_encrypted_store(state.clone(), &principal);
    let old_key = underwrap_backend_store::parse_hex_key(&old_key_hex).unwrap();

    let first = rotate_key_with_hooks(
        state.clone(),
        principal.clone(),
        store_id.clone(),
        &FixedHooks::new("1111111111111111", [10; 32]),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(first.key_hex, hex::encode([10; 32]));
    let first_workspace = workspace(&canonical, "1111111111111111");
    let first_snapshot = tree_snapshot(&first_workspace);

    let ignored = rotate_key_with_hooks(
        state.clone(),
        principal.clone(),
        store_id.clone(),
        &FixedHooks::new("2222222222222222", [11; 32]),
    )
    .await
    .unwrap();
    drop(ignored);
    assert_eq!(tree_snapshot(&first_workspace), first_snapshot);
    let second_workspace = workspace(&canonical, "2222222222222222");
    let second_snapshot = tree_snapshot(&second_workspace);

    let third = rotate_key_with_hooks(
        state,
        principal,
        store_id,
        &FixedHooks::new("3333333333333333", [12; 32]),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(third.key_hex, hex::encode([12; 32]));
    assert_eq!(tree_snapshot(&first_workspace), first_snapshot);
    assert_eq!(tree_snapshot(&second_workspace), second_snapshot);
    assert_generation(&first_workspace.join("backup"), old_key);
    assert_generation(&second_workspace.join("backup"), [10; 32]);
    assert_generation(
        &workspace(&canonical, "3333333333333333").join("backup"),
        [11; 32],
    );
    for path in [
        first_workspace,
        second_workspace,
        workspace(&canonical, "3333333333333333"),
        canonical,
    ] {
        assert_secure_encrypted_tree(&path);
    }
}
