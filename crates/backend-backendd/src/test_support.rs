//! Shared test helpers used by sibling `<module>/tests.rs` files
//! across the crate. Compiled only under `#[cfg(test)]`.
//!
//! Each helper here was lifted verbatim from the old monolithic
//! `mod tests` block in `server.rs`. They reach into production
//! modules directly (no `crate::server::*` indirection) so they can
//! be called from any per-module tests file with one
//! `use crate::test_support::*;` import.

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::policy::PolicyDocument;
use underwrap_protocol::service::{AdminStoreBackendKind, AdminStoreCreateRequest};
use underwrap_store::{StorageBackend, save_resource_blob};

use crate::admin_api::stores::do_admin_store_create;
use crate::runtime_state::AdminRuntimeState;
use crate::storage::documents::{
    load_pairs_document, load_policy_document, load_resources_document, save_pairs_document,
    save_policy_document, save_resources_document,
};
use crate::store_registry::{LoadedStoreRecord, StoreRegistry, StoreState};

#[allow(unused_imports)] // re-exported for convenience to callers
pub(crate) use crate::admin_api::csrf::AdminAuth;

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

pub(crate) fn test_users_file() -> PathBuf {
    std::env::temp_dir().join(format!(
        "underwrap-server-test-operators-{}.json",
        crate::users::new_user_id()
    ))
}

pub(crate) fn test_admin_state(
    stores: HashMap<String, Arc<dyn StorageBackend>>,
) -> AdminRuntimeState {
    let state: HashMap<String, StoreState> = stores
        .into_iter()
        .map(|(id, b)| (id, StoreState::Unlocked(b)))
        .collect();
    AdminRuntimeState {
        stores: Arc::new(std::sync::RwLock::new(StoreRegistry {
            state,
            records: HashMap::new(),
        })),
        client_instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
        stores_file: std::path::PathBuf::new(),
        client_instances_file: std::path::PathBuf::new(),
        stores_data_root: None,
        rotation_locks: std::sync::Mutex::new(HashMap::new()),
        admin_enabled: true,
        allowed_origins: None,
        web_source: crate::web::WebAssetSource::Embedded,
        users_file: test_users_file(),
        session_key: [0u8; crate::auth::SESSION_KEY_LEN],
        session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
        cookies_secure: false,
        login_rate_limiter: crate::auth::RateLimiter::new(),
        session_revocations: crate::auth::SessionRevocations::new(),
        bootstrap_lock: std::sync::Mutex::new(()),
        user_collection_lock: std::sync::Mutex::new(()),
    }
}

pub(crate) fn headers_with_session_cookie(value: Option<&str>) -> hyper::HeaderMap {
    let mut headers = hyper::HeaderMap::new();
    if let Some(v) = value {
        headers.insert(
            hyper::header::COOKIE,
            hyper::header::HeaderValue::from_str(&format!("__Host-uw-session={v}")).unwrap(),
        );
    }
    headers
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

pub(crate) fn seeded_admin_state(username: &str, password: &str) -> AdminRuntimeState {
    let users_file = test_users_file();
    let mut collection = crate::users::UserCollection::default();
    let hash = crate::auth::hash_password(password).unwrap();
    collection
        .add(crate::users::UserRecord {
            id: crate::users::new_user_id(),
            username: username.to_string(),
            password_hash: hash,
            created_at: 1000,
            password_changed_at: 1000,
        })
        .unwrap();
    crate::users::save(&users_file, &collection).unwrap();
    let mut session_key = [0u8; crate::auth::SESSION_KEY_LEN];
    for (i, b) in session_key.iter_mut().enumerate() {
        *b = i as u8;
    }
    AdminRuntimeState {
        stores: Arc::new(std::sync::RwLock::new(StoreRegistry::new())),
        client_instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
        stores_file: std::path::PathBuf::new(),
        client_instances_file: std::path::PathBuf::new(),
        stores_data_root: None,
        rotation_locks: std::sync::Mutex::new(HashMap::new()),
        admin_enabled: true,
        allowed_origins: None,
        web_source: crate::web::WebAssetSource::Embedded,
        users_file,
        session_key,
        session_lifetime_secs: 3600,
        cookies_secure: false,
        login_rate_limiter: crate::auth::RateLimiter::new(),
        session_revocations: crate::auth::SessionRevocations::new(),
        bootstrap_lock: std::sync::Mutex::new(()),
        user_collection_lock: std::sync::Mutex::new(()),
    }
}

pub(crate) fn session_headers(
    state: &AdminRuntimeState,
    username: &str,
    with_csrf_cookie: bool,
    csrf_header: Option<&str>,
) -> (hyper::HeaderMap, crate::auth::SessionPayload) {
    let key = state.session_key;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let payload = crate::auth::new_session_payload("op-1".into(), username.into(), now, 3600);
    let session_cookie = crate::auth::encrypt_session(&payload, &key).unwrap();
    let mut cookie_parts = vec![format!("__Host-uw-session={session_cookie}")];
    if with_csrf_cookie {
        cookie_parts.push(format!("__Host-uw-csrf={}", payload.csrf_secret));
    }
    let mut headers = hyper::HeaderMap::new();
    headers.insert(
        hyper::header::COOKIE,
        hyper::header::HeaderValue::from_str(&cookie_parts.join("; ")).unwrap(),
    );
    if let Some(value) = csrf_header {
        headers.insert(
            hyper::header::HeaderName::from_static("x-csrf-token"),
            hyper::header::HeaderValue::from_str(value).unwrap(),
        );
    }
    (headers, payload)
}

pub(crate) fn service_state_with_runtime_client(
    cert_der: &[u8],
    status: crate::runtime_clients::ClientInstanceStatus,
    store_id: &str,
) -> AdminRuntimeState {
    let mut state_map: HashMap<String, StoreState> = HashMap::new();
    state_map.insert(
        store_id.to_string(),
        StoreState::Unlocked(Arc::new(MemTestStore::new())),
    );
    let mut records: HashMap<String, LoadedStoreRecord> = HashMap::new();
    records.insert(
        store_id.to_string(),
        LoadedStoreRecord {
            record: underwrap_protocol::config::StoreRecord {
                id: store_id.to_string(),
                name: store_id.to_string(),
                description: None,
                created_at: None,
                storage: underwrap_protocol::config::StorageConfig::Plain {
                    dir: PathBuf::from("/dev/null"),
                },
            },
        },
    );
    let mut client_instances = HashMap::new();
    client_instances.insert(
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
                issued_at: None,
            },
            auto_create_wrapper_assets: false,
        },
    );
    AdminRuntimeState {
        stores: Arc::new(std::sync::RwLock::new(StoreRegistry {
            state: state_map,
            records,
        })),
        client_instances: Arc::new(std::sync::RwLock::new(client_instances)),
        stores_file: PathBuf::new(),
        client_instances_file: PathBuf::new(),
        stores_data_root: None,
        rotation_locks: std::sync::Mutex::new(HashMap::new()),
        admin_enabled: true,
        allowed_origins: None,
        web_source: crate::web::WebAssetSource::Embedded,
        users_file: test_users_file(),
        session_key: [0u8; crate::auth::SESSION_KEY_LEN],
        session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
        cookies_secure: false,
        login_rate_limiter: crate::auth::RateLimiter::new(),
        session_revocations: crate::auth::SessionRevocations::new(),
        bootstrap_lock: std::sync::Mutex::new(()),
        user_collection_lock: std::sync::Mutex::new(()),
    }
}

pub(crate) fn expect_404(
    result: std::result::Result<Arc<dyn StorageBackend>, Response<Full<Bytes>>>,
) {
    match result {
        Ok(_) => panic!("expected 404 not_found, got Ok"),
        Err(resp) => assert_eq!(resp.status(), StatusCode::NOT_FOUND),
    }
}

pub(crate) fn fresh_bootstrap_state() -> AdminRuntimeState {
    let mut key = [0u8; crate::auth::SESSION_KEY_LEN];
    for (i, b) in key.iter_mut().enumerate() {
        *b = i as u8;
    }
    AdminRuntimeState {
        stores: Arc::new(std::sync::RwLock::new(StoreRegistry::new())),
        client_instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
        stores_file: std::path::PathBuf::new(),
        client_instances_file: std::path::PathBuf::new(),
        stores_data_root: None,
        rotation_locks: std::sync::Mutex::new(HashMap::new()),
        admin_enabled: true,
        allowed_origins: None,
        web_source: crate::web::WebAssetSource::Embedded,
        users_file: test_users_file(),
        session_key: key,
        session_lifetime_secs: 3600,
        cookies_secure: false,
        login_rate_limiter: crate::auth::RateLimiter::new(),
        session_revocations: crate::auth::SessionRevocations::new(),
        bootstrap_lock: std::sync::Mutex::new(()),
        user_collection_lock: std::sync::Mutex::new(()),
    }
}

pub(crate) async fn read_json(resp: Response<Full<Bytes>>) -> serde_json::Value {
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    serde_json::from_slice(&body).unwrap()
}

/// Drive the bootstrap-flow inner logic (everything after the JSON
/// body has been parsed). We can't synthesise a `Request<Incoming>`
/// in unit tests, so factor the core into a helper exercised by
/// tests against the real handler.
pub(crate) fn run_bootstrap(
    state: &AdminRuntimeState,
    username: &str,
    password: &str,
) -> Response<Full<Bytes>> {
    use crate::admin_api::bootstrap::audit_bootstrap_attempt;
    use crate::http::responses::error_response;
    // Mirror the validation in `handle_admin_bootstrap` minus the
    // request-body parsing, which `parse_json_body` already covers.
    let session_key = state.session_key;
    let username_norm = crate::users::normalise_username(username);
    if username_norm.is_empty() || password.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_credentials",
            "missing field",
        );
    }
    let _guard = state.bootstrap_lock.lock().unwrap();
    if crate::users::bootstrap_completed(&state.users_file).unwrap_or(true) {
        audit_bootstrap_attempt(state, &username_norm, "bootstrap_already_completed");
        return error_response(
            StatusCode::CONFLICT,
            "bootstrap_already_completed",
            "already done",
        );
    }
    let collection = crate::users::load(&state.users_file).unwrap();
    if !collection.is_empty() {
        audit_bootstrap_attempt(state, &username_norm, "users_already_exist");
        return error_response(
            StatusCode::CONFLICT,
            "users_already_exist",
            "already populated",
        );
    }
    let now = crate::users::now_epoch_secs();
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
    crate::users::save(&state.users_file, &new_collection).unwrap();
    crate::users::mark_bootstrap_completed(
        &state.users_file,
        &crate::users::BootstrapSentinel {
            completed_at: now,
            by_username: record.username.clone(),
        },
    )
    .unwrap();
    audit_bootstrap_attempt(state, &record.username, "success");
    let payload = crate::auth::new_session_payload(
        record.id.clone(),
        record.username.clone(),
        now,
        state.session_lifetime_secs,
    );
    let encoded = crate::auth::encrypt_session(&payload, &session_key).unwrap();
    let session_cookie = crate::auth::render_session_cookie(
        &encoded,
        state.session_lifetime_secs,
        state.cookies_secure,
    );
    let csrf_cookie = crate::auth::render_csrf_cookie(
        &payload.csrf_secret,
        state.session_lifetime_secs,
        state.cookies_secure,
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .header(hyper::header::SET_COOKIE, session_cookie)
        .header(hyper::header::SET_COOKIE, csrf_cookie)
        .body(Full::new(Bytes::from_static(b"{}")))
        .expect("response")
}

pub(crate) fn admin_principal(_stores: &[&str]) -> crate::users::Principal {
    crate::users::Principal {
        id: "admin-id".into(),
        username: "alice".into(),
    }
}

/// Build an `AdminRuntimeState` wired up for CRUD tests: a
/// dedicated store registry, user-state file, and one allowed
/// data root, all under the supplied tempdir.
pub(crate) fn crud_state(tmp: &std::path::Path) -> AdminRuntimeState {
    let dynamic_dir = tmp.join("stores.d");
    std::fs::create_dir_all(&dynamic_dir).unwrap();
    let data_root = tmp.join("data");
    std::fs::create_dir_all(&data_root).unwrap();
    AdminRuntimeState {
        stores: Arc::new(std::sync::RwLock::new(StoreRegistry::new())),
        client_instances: Arc::new(std::sync::RwLock::new(HashMap::new())),
        stores_file: tmp.join("stores.json"),
        client_instances_file: tmp.join("client-instances.json"),
        stores_data_root: Some(data_root),
        rotation_locks: std::sync::Mutex::new(HashMap::new()),
        admin_enabled: true,
        allowed_origins: None,
        web_source: crate::web::WebAssetSource::Embedded,
        users_file: tmp.join("users.json"),
        session_key: [0u8; crate::auth::SESSION_KEY_LEN],
        session_lifetime_secs: crate::auth::DEFAULT_SESSION_LIFETIME_SECS,
        cookies_secure: false,
        login_rate_limiter: crate::auth::RateLimiter::new(),
        session_revocations: crate::auth::SessionRevocations::new(),
        bootstrap_lock: std::sync::Mutex::new(()),
        user_collection_lock: std::sync::Mutex::new(()),
    }
}

pub(crate) fn create_store_for_test(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: AdminStoreCreateRequest,
) -> String {
    let resp = do_admin_store_create(state, principal, body);
    assert_eq!(resp.status(), StatusCode::OK);
    response_body_json(resp)["store"]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

pub(crate) fn response_body_json(resp: Response<Full<Bytes>>) -> serde_json::Value {
    let (parts, body) = resp.into_parts();
    let bytes = body.collect_bytes_simple();
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("response body is JSON");
    // Smuggle the status into the JSON value so the test can assert both.
    let mut obj = v.as_object().cloned().unwrap_or_default();
    obj.insert(
        "__status".into(),
        serde_json::Value::Number(parts.status.as_u16().into()),
    );
    serde_json::Value::Object(obj)
}

// Local trait extension so we can grab the body bytes synchronously.
pub(crate) trait CollectBytesSync {
    fn collect_bytes_simple(self) -> Vec<u8>;
}
impl CollectBytesSync for Full<Bytes> {
    fn collect_bytes_simple(self) -> Vec<u8> {
        use http_body_util::BodyExt;
        // If we're already on a tokio runtime (e.g. inside a
        // `#[tokio::test(flavor = "multi_thread")]` test) we cannot
        // construct a nested Runtime — use `block_in_place` to
        // escape the worker. Otherwise spin up a small runtime
        // just for this drain.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            tokio::task::block_in_place(|| {
                handle.block_on(async { self.collect().await.unwrap().to_bytes().to_vec() })
            })
        } else {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async { self.collect().await.unwrap().to_bytes().to_vec() })
        }
    }
}

pub(crate) fn entity_test_state_with_store(tmp: &std::path::Path) -> (AdminRuntimeState, String) {
    let state = crud_state(tmp);
    let p = admin_principal(&[]);
    let id = create_store_for_test(
        &state,
        &p,
        AdminStoreCreateRequest {
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
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    store_id: &str,
    name: &str,
) -> String {
    let resp = crate::admin_api::resources::handle_admin_resource_create(
        state,
        principal,
        &underwrap_protocol::service::AdminResourceCreateRequest {
            store: store_id.to_string(),
            name: name.to_string(),
        },
    );
    assert_eq!(resp.status(), StatusCode::OK);
    response_body_json(resp)["resource"]["id"]
        .as_str()
        .expect("create response includes resource.id")
        .to_string()
}

pub(crate) fn seed_admin_into(state: &AdminRuntimeState, username: &str) -> String {
    // Returns the operator id of the seeded admin so tests can
    // build a principal that refers to a real record.
    let mut c = crate::users::UserCollection::default();
    let hash = crate::auth::hash_password("seed-pass").unwrap();
    let now = crate::users::now_epoch_secs();
    let id = crate::users::new_user_id();
    c.add(crate::users::UserRecord {
        id: id.clone(),
        username: username.into(),
        password_hash: hash,
        created_at: now,
        password_changed_at: now,
    })
    .unwrap();
    crate::users::save(&state.users_file, &c).unwrap();
    id
}

pub(crate) fn admin_principal_with_id(id: &str) -> crate::users::Principal {
    crate::users::Principal {
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
    pub(crate) state: AdminRuntimeState,
    pub(crate) store_id: String,
    pub(crate) instance_id: String,
    pub(crate) resource_id: String,
}

impl RuntimeApiFixture {
    pub(crate) fn store(&self) -> Arc<dyn StorageBackend> {
        self.state
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
    let state = crud_state(tmp.path());
    let p = admin_principal(&[]);
    let store_id = create_store_for_test(
        &state,
        &p,
        AdminStoreCreateRequest {
            name: "work".into(),
            backend: AdminStoreBackendKind::Plain,
            description: None,
        },
    );

    let instance_id = "inst-runtime".to_string();
    {
        let mut instances = state.client_instances.write().unwrap();
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
                    issued_at: Some(1),
                },
                auto_create_wrapper_assets: false,
            },
        );
        crate::runtime_clients::save_client_instances(&state.client_instances_file, &instances)
            .unwrap();
    }

    let store = state.stores.read().unwrap().expect_unlocked(&store_id);
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
