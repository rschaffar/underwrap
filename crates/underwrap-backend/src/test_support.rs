//! Shared test helpers used by sibling `<module>/tests.rs` files
//! across the crate. Compiled only under `#[cfg(test)]`.
//!
//! Helpers reach into production modules directly (no
//! `crate::server::*` indirection) so they can be called from any
//! per-module tests file with one `use crate::test_support::*;`
//! import.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::response::IntoResponse;
use hyper::StatusCode;
use underwrap_backend_store::{StorageBackend, save_resource_blob};
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::policy::PolicyDocument;
use underwrap_protocol::service::{AdminStoreBackendKind, AdminStoreCreateRequest};

use crate::admin_api::stores::crud::handle_admin_store_create;
use crate::runtime_state::{AdminState, ListenerState, SharedState};
use crate::storage::documents::{
    load_pairs_document, load_policy_document, load_resources_document, save_pairs_document,
    save_policy_document, save_resources_document,
};
use crate::store_registry::{StoreEntry, StoreRegistry, StoreState};

/// A sample client cert in PEM form used by tests that need to seed
/// a runtime-client record without going through the create API
/// (which validates the PEM).
pub(crate) const SAMPLE_CLIENT_CERT_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDKzCCAhOgAwIBAgIUVnjkCKXzQiyHyEsQ3ey4lgWkrTAwDQYJKoZIhvcNAQEL
BQAwHDEaMBgGA1UEAwwRdW5kZXJ3cmFwLXRlc3QtY2EwHhcNMjYwNDE2MDAzNzAy
WhcNMzYwNDEzMDAzNzAyWjAaMRgwFgYDVQQDDA91bmRlcndyYXAtbG9jYWwwggEi
MA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQDcJDP5K69TcG9+uHM7q+xG7p0c
7Cgex8XYVs3yFDpHk6UOG/JqL0bKa32rhO7LR8VxPprIyqV24LB8vTi52A2/oA/d
ZiVouv9bkCiBOun0XDQAHveocrIYnhjvq8qrpdfFdZ+2oB/V78Mbf6FaNMJxvvUN
hCRXh/hstwlHVACJFX0MlGU/3WAhpRF09QjIfxLUqm1t5Sd9Qg0a2IsX+qrFhDiu
35anBoCx0x9AJl6yO40KgoDZ7iDmWLw55T2GVnSQEdTelxzsf9jFLjEzm56mymR+
DGWdMNPEKY6wyvuJClEYLEsHaNMfTi9Jp+9FgH5spuy7E9UqpqeciYAHlDrxAgMB
AAGjZzBlMBMGA1UdJQQMMAoGCCsGAQUFBwMCMA4GA1UdDwEB/wQEAwIFoDAdBgNV
HQ4EFgQUrZfzoVv+iUwAlCedd1/ancfUi7YwHwYDVR0jBBgwFoAUlScjoHF7mIGw
Qy61SLb9isP1dd4wDQYJKoZIhvcNAQELBQADggEBAMj5BtbOalwYWAi9HQt0JNPq
NtRObdNx6Yg2uZWYAyhVXnhLerZ82hWagK/oH0KCgKFCSK140oZS/Nt7xtQ9YJXs
g5WyGnsteF3QZ9cuUc+0te4eMOmXRMch8HtxyD1sCuO/wyKhtQSCO+OQGNvsEnKa
1rl3PFTjG3SE5ldDwRjGSlrf9+6uf2OcqJBLaL4dcOJ0Fsrf0gkkPpbrC5hJdNSo
Q+0W8oyLdzgmGNMRY4vvhu4cjYkbPLgrNS3fQLnnom3Jv5GzO3M2ifS9mkj249Bu
cFuCWqBWazuUyiSP6kQz7Ad4/9qxEu4c8Hv3Ecc5XuQDpl4Rxox+j/7lUrthzrI=
-----END CERTIFICATE-----
"#;

// ===== Storage backends used in tests =====

#[derive(Default)]
pub(crate) struct MemTestStore {
    pub(crate) data: std::sync::Mutex<HashMap<String, Vec<u8>>>,
}

impl MemTestStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

impl StorageBackend for MemTestStore {
    fn read(&self, id: &str) -> anyhow::Result<Option<underwrap_protocol::message::SecretBytes>> {
        Ok(self.data.lock().unwrap().get(id).cloned().map(Into::into))
    }
    fn write(&self, id: &str, m: &[u8]) -> anyhow::Result<()> {
        self.data.lock().unwrap().insert(id.to_string(), m.to_vec());
        Ok(())
    }
    fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }
    fn delete(&self, id: &str) -> anyhow::Result<bool> {
        Ok(self.data.lock().unwrap().remove(id).is_some())
    }
}

pub(crate) struct TestStore {
    pub(crate) data: std::sync::Mutex<HashMap<String, Vec<u8>>>,
    pub(crate) fail_ops: std::sync::Mutex<std::collections::HashSet<&'static str>>,
}

impl TestStore {
    pub(crate) fn new() -> Self {
        Self {
            data: std::sync::Mutex::new(HashMap::new()),
            fail_ops: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    fn should_fail(&self, op: &'static str) -> anyhow::Result<()> {
        if self.fail_ops.lock().unwrap().contains(op) {
            anyhow::bail!("forced {op} failure");
        }
        Ok(())
    }
}

impl StorageBackend for TestStore {
    fn read(&self, secret_id: &str) -> anyhow::Result<Option<SecretBytes>> {
        self.should_fail("read")?;
        Ok(self
            .data
            .lock()
            .unwrap()
            .get(secret_id)
            .cloned()
            .map(Into::into))
    }

    fn write(&self, secret_id: &str, material: &[u8]) -> anyhow::Result<()> {
        self.should_fail("write")?;
        self.data
            .lock()
            .unwrap()
            .insert(secret_id.to_string(), material.to_vec());
        Ok(())
    }

    fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        self.should_fail("list")?;
        let mut ids: Vec<_> = self
            .data
            .lock()
            .unwrap()
            .keys()
            .filter(|id| id.starts_with(prefix))
            .cloned()
            .collect();
        ids.sort();
        Ok(ids)
    }

    fn delete(&self, secret_id: &str) -> anyhow::Result<bool> {
        self.should_fail("delete")?;
        Ok(self.data.lock().unwrap().remove(secret_id).is_some())
    }
}

// ===== Misc helpers =====

/// Builds a fresh `(users_dir, bootstrap_file)` pair under the process
/// temp dir for tests that need somewhere to persist user records.
/// The directory itself is created lazily by `save_user` when the
/// first record is written.
pub(crate) fn test_users_paths() -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!(
        "underwrap-server-test-operators-{}",
        crate::users::new_user_id()
    ));
    let users_dir = base.join("users");
    let bootstrap_file = base.join("bootstrap.json");
    (users_dir, bootstrap_file)
}

/// Canonical "blank" ListenerState: empty registry, no client
/// instances, admin enabled, embedded SPA, fresh in-memory session
/// store, a fresh temporary users directory + bootstrap file. Every
/// named builder below starts here and mutates the one or two fields
/// it actually cares about.
pub(crate) fn default_admin_runtime_state() -> ListenerState {
    let (users_dir, bootstrap_file) = test_users_paths();
    let cookies_secure = false;
    ListenerState {
        shared: SharedState {
            stores: Arc::new(std::sync::RwLock::new(StoreRegistry::default())),
            client_instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
        },
        admin: Some(AdminState {
            stores_dir: PathBuf::new(),
            clients_dir: PathBuf::new(),
            stores_data_root: None,
            allowed_origins: Vec::new(),
            users_dir: users_dir.clone(),
            bootstrap_file,
            auth_backend: crate::auth::AuthBackend::new(users_dir),
            session_store: tower_sessions::MemoryStore::default(),
            csrf_config: crate::admin_api::csrf::build_csrf_config(cookies_secure),
            session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
            cookies_secure,
            bootstrap_lock: std::sync::Mutex::new(()),
            user_collection_lock: std::sync::Mutex::new(()),
        }),
    }
}

/// Borrow the AdminState mutably from a `ListenerState` built by one of
/// the test helpers below. Panics if the state was built without admin —
/// the test helpers always set it, so the unwrap is for programming
/// errors, not run-time conditions.
pub(crate) fn admin_mut(state: &mut ListenerState) -> &mut AdminState {
    state
        .admin
        .as_mut()
        .expect("test ListenerState lacks AdminState")
}

pub(crate) fn test_admin_state(stores: HashMap<String, Arc<dyn StorageBackend>>) -> ListenerState {
    let entries: HashMap<String, StoreEntry> = stores
        .into_iter()
        .map(|(id, b)| {
            let record = underwrap_protocol::config::StoreRecord {
                id: id.clone(),
                name: id.clone(),
                description: None,
                created_at: None,
                storage: underwrap_protocol::config::StorageConfig::Plain {
                    dir: PathBuf::from("/dev/null"),
                },
            };
            (id, StoreEntry::new(record, StoreState::Unlocked(b)))
        })
        .collect();
    let mut s = default_admin_runtime_state();
    s.shared.stores = Arc::new(std::sync::RwLock::new(StoreRegistry { entries }));
    s
}

pub(crate) fn headers_with(pairs: &[(&str, &str)]) -> hyper::HeaderMap {
    let mut headers = hyper::HeaderMap::new();
    for (k, v) in pairs {
        headers.insert(
            hyper::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
            hyper::header::HeaderValue::from_str(v).unwrap(),
        );
    }
    headers
}

pub(crate) fn seeded_admin_state(username: &str, password: &str) -> ListenerState {
    let mut s = default_admin_runtime_state();
    admin_mut(&mut s).session_lifetime_secs = 3600;
    let hash = crate::auth::hash_password(password).unwrap();
    let record = crate::users::UserRecord {
        id: crate::users::new_user_id(),
        username: username.to_string(),
        password_hash: hash,
        created_at: 1000,
        password_changed_at: 1000,
    };
    // Run through the collection invariants once for parity with the
    // production paths, then write the single record to disk.
    let mut collection = crate::users::UserCollection::default();
    collection.add(record.clone()).unwrap();
    let users_dir = admin_mut(&mut s).users_dir.clone();
    crate::users::save_user(&users_dir, &record).unwrap();
    // Rebuild the `AuthBackend` so it points at the now-written users
    // directory. The backend reads the dir from disk on every
    // `authenticate` / `get_user` call, but the path is captured at
    // construction time, so re-pointing it here makes login tests
    // resolve against the freshly-seeded record.
    admin_mut(&mut s).auth_backend = crate::auth::AuthBackend::new(users_dir);
    s
}

pub(crate) fn service_state_with_runtime_client(
    cert_der: &[u8],
    status: crate::runtime_clients::ClientInstanceStatus,
    store_id: &str,
) -> ListenerState {
    let record = underwrap_protocol::config::StoreRecord {
        id: store_id.to_string(),
        name: store_id.to_string(),
        description: None,
        created_at: None,
        storage: underwrap_protocol::config::StorageConfig::Plain {
            dir: PathBuf::from("/dev/null"),
        },
    };
    let entries = HashMap::from([(
        store_id.to_string(),
        StoreEntry::new(record, StoreState::Unlocked(Arc::new(MemTestStore::new()))),
    )]);
    let client_instances = HashMap::from([(
        "inst_local".to_string(),
        crate::runtime_clients::ClientInstanceRecord {
            id: "inst_local".to_string(),
            display_name: "Local".to_string(),
            current_store_id: store_id.to_string(),
            status,
            last_seen_at: None,
            current_credential: crate::runtime_clients::CurrentCredentialRecord {
                cert_fingerprint: crate::runtime_clients::cert_fingerprint_from_der(cert_der),
                cert_pem: "pem".to_string(),
            },
            auto_create_wrapper_assets: false,
        },
    )]);
    let mut s = default_admin_runtime_state();
    s.shared.stores = Arc::new(std::sync::RwLock::new(StoreRegistry { entries }));
    s.shared.client_instances = Arc::new(std::sync::RwLock::new(client_instances));
    s
}

pub(crate) fn expect_404<T, E: IntoResponse>(result: std::result::Result<T, E>) {
    match result {
        Ok(_) => panic!("expected 404 not_found, got Ok"),
        Err(err) => {
            let resp = err.into_response();
            assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        }
    }
}

pub(crate) fn fresh_bootstrap_state() -> ListenerState {
    let mut s = default_admin_runtime_state();
    admin_mut(&mut s).session_lifetime_secs = 3600;
    s
}

/// Run a future to completion from a synchronous test, using the
/// current tokio runtime if any (e.g. `#[tokio::test]`) or spinning
/// up a tiny one-shot runtime otherwise.
pub(crate) fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(fut))
    } else {
        tokio::runtime::Runtime::new().unwrap().block_on(fut)
    }
}

/// Drive an admin handler future to completion and collapse it to a
/// `(status, body_json)` pair. The handler return type only needs to
/// be `IntoResponse`, so `Result<Json<T>, AdminError>` and bare
/// `Json<T>` both work without extra ceremony at the call site.
pub(crate) fn run_handler<F>(fut: F) -> (StatusCode, serde_json::Value)
where
    F: std::future::Future,
    F::Output: IntoResponse,
{
    let response = block_on(fut).into_response();
    let (parts, body) = response.into_parts();
    let bytes = block_on(async move {
        axum::body::to_bytes(body, 8 * 1024 * 1024)
            .await
            .expect("collect response body")
    });
    let value: serde_json::Value = if bytes.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            panic!(
                "response body was not JSON ({} bytes): {e}; raw = {:?}",
                bytes.len(),
                String::from_utf8_lossy(&bytes)
            )
        })
    };
    (parts.status, value)
}

/// Mirror of the production `handle_admin_bootstrap` validation +
/// state mutations, minus the body-parse and AuthSession-issue steps.
/// Used by tests that only care about the DB-side state transition
/// and don't want to spin up an `AdminAuthSession`.
pub(crate) fn run_bootstrap(
    state: &ListenerState,
    username: &str,
    password: &str,
) -> (StatusCode, serde_json::Value) {
    use crate::admin_api::AdminError;
    use crate::admin_api::audit::log_bootstrap_attempt;

    let username_norm = crate::users::normalise_username(username);
    if username_norm.is_empty() {
        let err = AdminError::bad_request("invalid_credentials", "missing username");
        return run_handler(async { Err::<axum::Json<()>, AdminError>(err) });
    }
    let _guard = state.admin().bootstrap_lock.lock().unwrap();
    if crate::users::bootstrap_completed(&state.admin().bootstrap_file).unwrap_or(true) {
        log_bootstrap_attempt(
            &username_norm,
            Some("bootstrap-already-completed"),
            "failure",
        );
        let err = AdminError::conflict("bootstrap_already_completed", "already done");
        return run_handler(async { Err::<axum::Json<()>, AdminError>(err) });
    }
    let collection = crate::users::load(&state.admin().users_dir).unwrap();
    if !collection.is_empty() {
        log_bootstrap_attempt(&username_norm, Some("users-already-exist"), "failure");
        let err = AdminError::conflict("users_already_exist", "already populated");
        return run_handler(async { Err::<axum::Json<()>, AdminError>(err) });
    }
    if let Err(code) = crate::auth::validate_password(password) {
        log_bootstrap_attempt(&username_norm, Some(code), "failure");
        let err = AdminError::bad_request(code, "password rejected");
        return run_handler(async { Err::<axum::Json<()>, AdminError>(err) });
    }
    let now = crate::time::now_epoch_secs();
    let hash = crate::auth::hash_password(password).unwrap();
    let record = crate::users::UserRecord {
        id: crate::users::new_user_id(),
        username: username_norm.clone(),
        password_hash: hash,
        created_at: now,
        password_changed_at: now,
    };
    let mut new_collection = crate::users::UserCollection::default();
    new_collection.add(record.clone()).unwrap();
    crate::users::save_user(&state.admin().users_dir, &record).unwrap();
    crate::users::mark_bootstrap_completed(&state.admin().bootstrap_file).unwrap();
    log_bootstrap_attempt(&record.username, None, "success");
    let body = crate::admin_api::auth::bootstrap::BootstrapResponse {
        username: record.username.clone(),
    };
    run_handler(async { Ok::<_, crate::admin_api::AdminError>(axum::Json(body)) })
}

pub(crate) fn admin_principal() -> crate::admin_api::csrf::Principal {
    crate::admin_api::csrf::Principal {
        id: "admin-id".into(),
        username: "alice".into(),
    }
}

/// Build an `ListenerState` wired up for CRUD tests: a
/// dedicated store registry, user-state file, and one allowed
/// data root, all under the supplied tempdir.
pub(crate) fn crud_state(tmp: &std::path::Path) -> ListenerState {
    let dynamic_dir = tmp.join("stores.d");
    std::fs::create_dir_all(&dynamic_dir).unwrap();
    let data_root = tmp.join("data");
    std::fs::create_dir_all(&data_root).unwrap();
    let mut s = default_admin_runtime_state();
    {
        let admin = admin_mut(&mut s);
        admin.stores_dir = tmp.join("stores");
        admin.clients_dir = tmp.join("clients");
        admin.stores_data_root = Some(data_root);
        admin.users_dir = tmp.join("users");
        admin.bootstrap_file = tmp.join("bootstrap.json");
        // Point the AuthBackend at the new tempdir-relative users dir;
        // `default_admin_runtime_state` initialised it against the
        // process-wide test temp dir we have now overridden.
        admin.auth_backend = crate::auth::AuthBackend::new(admin.users_dir.clone());
    }
    s
}

/// Create a store via the admin handler and return its assigned id.
///
/// The handler runs through its full Axum extractor surface here so we
/// hit the real code path, but the helper hides the extractor wrapping
/// from per-module tests that just want "give me a store id".
pub(crate) fn create_store_for_test(
    state: Arc<ListenerState>,
    principal: &crate::admin_api::csrf::Principal,
    body: &AdminStoreCreateRequest,
) -> String {
    let (status, value) = run_handler(handle_admin_store_create(
        axum::extract::State(state),
        axum::Extension(principal.clone()),
        crate::http::responses::Body(body.clone()),
    ));
    assert_eq!(status, StatusCode::OK, "store create failed: {value}");
    value["store"]["id"].as_str().unwrap().to_string()
}

/// Sync helper used by `runtime_api/*` tests. Accepts any `IntoResponse`, turns
/// it into an `axum::response::Response`, drains the body, and decodes it as
/// JSON. The HTTP status is smuggled under `__status` so a single
/// `assert_eq!` can check both shape and code. Admin-side tests use
/// [`run_handler`] instead.
pub(crate) fn response_body_json(resp: impl axum::response::IntoResponse) -> serde_json::Value {
    use http_body_util::BodyExt;
    let resp = resp.into_response();
    let (parts, body) = resp.into_parts();
    let bytes = block_on(async { body.collect().await.unwrap().to_bytes().to_vec() });
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("response body is JSON");
    let mut obj = v.as_object().cloned().unwrap_or_default();
    obj.insert(
        "__status".into(),
        serde_json::Value::Number(parts.status.as_u16().into()),
    );
    serde_json::Value::Object(obj)
}

pub(crate) fn entity_test_state_with_store(tmp: &std::path::Path) -> (Arc<ListenerState>, String) {
    let state = Arc::new(crud_state(tmp));
    let p = admin_principal();
    let id = create_store_for_test(
        state.clone(),
        &p,
        &AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );
    (state, id)
}

/// Test helper: call the resource create handler and return the
/// id the backend assigned. Tests use this when they need to issue
/// subsequent id-addressed calls against the same resource.
pub(crate) fn create_resource_id_for_test(
    state: Arc<ListenerState>,
    principal: &crate::admin_api::csrf::Principal,
    store_id: &str,
    name: &str,
) -> String {
    let (status, value) = run_handler(crate::admin_api::resources::handle_admin_resource_create(
        axum::extract::State(state),
        axum::Extension(principal.clone()),
        axum::extract::Path(store_id.to_string()),
        crate::http::responses::Body(underwrap_protocol::service::AdminResourceCreateRequest {
            name: name.to_string(),
        }),
    ));
    assert_eq!(status, StatusCode::OK, "resource create failed: {value}");
    value["resource"]["id"]
        .as_str()
        .expect("create response includes resource.id")
        .to_string()
}

pub(crate) fn seed_admin_into(state: &ListenerState, username: &str) -> String {
    // Returns the operator id of the seeded admin so tests can
    // build a principal that refers to a real record.
    let hash = crate::auth::hash_password("seed-pass").unwrap();
    let now = crate::time::now_epoch_secs();
    let id = crate::users::new_user_id();
    let record = crate::users::UserRecord {
        id: id.clone(),
        username: username.into(),
        password_hash: hash,
        created_at: now,
        password_changed_at: now,
    };
    let mut c = crate::users::UserCollection::default();
    c.add(record.clone()).unwrap();
    crate::users::save_user(&state.admin().users_dir, &record).unwrap();
    id
}

pub(crate) fn admin_principal_with_id(id: &str) -> crate::admin_api::csrf::Principal {
    crate::admin_api::csrf::Principal {
        id: id.into(),
        username: "alice".into(),
    }
}

/// Fixture for the new runtime API tests. Builds a `crud_state`
/// with a plain store containing one resource ("alpha", id derived
/// from the catalog), one `(gh, alpha)` pair, and one Unlocked
/// writable permission for `instance_id`. Returns enough info for
/// individual tests to issue the new runtime endpoints.
pub(crate) struct RuntimeApiFixture {
    pub(crate) _tmp: tempfile::TempDir,
    pub(crate) state: Arc<ListenerState>,
    pub(crate) store_id: String,
    pub(crate) instance_id: String,
    pub(crate) resource_id: String,
}

impl RuntimeApiFixture {
    pub(crate) fn store(&self) -> Arc<dyn StorageBackend> {
        self.state
            .shared
            .stores
            .read()
            .unwrap()
            .expect_unlocked(&self.store_id)
    }

    pub(crate) fn policy(&self) -> PolicyDocument {
        load_policy_document(self.store().as_ref()).unwrap()
    }

    pub(crate) fn save_policy(&self, doc: &PolicyDocument) {
        save_policy_document(self.store().as_ref(), doc).unwrap();
    }
}

pub(crate) fn make_runtime_api_fixture() -> RuntimeApiFixture {
    use underwrap_protocol::entities::{EntityCreator, ResourceRecord, WrapperResourcePairRecord};
    use underwrap_protocol::policy::{WrapperApprovalMode, WrapperPermission};
    use underwrap_protocol::resource_blob::ResourceBlob;

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
        },
    );

    let instance_id = "inst-runtime".to_string();
    {
        let mut instances = state.shared.client_instances.write().unwrap();
        instances.insert(
            instance_id.clone(),
            crate::runtime_clients::ClientInstanceRecord {
                id: instance_id.clone(),
                display_name: "test".into(),
                current_store_id: store_id.clone(),
                status: crate::runtime_clients::ClientInstanceStatus::Active,
                last_seen_at: None,
                current_credential: crate::runtime_clients::CurrentCredentialRecord {
                    cert_fingerprint: "fp-runtime".into(),
                    cert_pem: SAMPLE_CLIENT_CERT_PEM.into(),
                },
                auto_create_wrapper_assets: false,
            },
        );
        for record in instances.values() {
            crate::runtime_clients::save_client_instance(&state.admin().clients_dir, record)
                .unwrap();
        }
    }

    let store = state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(&store_id);
    let resource_id = "res_alpha".to_string();
    let mut resources = load_resources_document(store.as_ref()).unwrap();
    resources
        .insert(ResourceRecord {
            id: resource_id.clone(),
            name: "alpha".into(),
            created_by: EntityCreator::Operator {
                id: "admin-id".into(),
                username: "alice".into(),
            },
            created_at: 1,
        })
        .unwrap();
    save_resources_document(store.as_ref(), &resources).unwrap();
    let mut pairs = load_pairs_document(store.as_ref()).unwrap();
    pairs
        .insert(WrapperResourcePairRecord {
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
            created_by: EntityCreator::ClientInstance {
                id: instance_id.clone(),
            },
            created_at: 1,
            last_used_at: None,
        })
        .unwrap();
    save_pairs_document(store.as_ref(), &pairs).unwrap();

    let policy = PolicyDocument {
        permissions: vec![WrapperPermission {
            client_instance: instance_id.clone(),
            wrapper: "gh".into(),
            resource_id: resource_id.clone(),
            mode: WrapperApprovalMode::Unlocked,
            writable: true,
            reason: None,
        }],
    };
    save_policy_document(store.as_ref(), &policy).unwrap();
    save_resource_blob(store.as_ref(), &resource_id, &ResourceBlob::empty()).unwrap();

    RuntimeApiFixture {
        _tmp: tmp,
        state,
        store_id,
        instance_id,
        resource_id,
    }
}
