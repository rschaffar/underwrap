//! Backend daemon server.
//!
//! Exposes a management socket used to dynamically export named stores onto
//! Unix sockets, and serves the backend data protocol on those exported sockets.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, Notify};
use tokio::task::{AbortHandle, JoinSet};
use tracing::{error, info};

use underwrap_protocol::config::StorageConfig;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{
    BackendErrorCode, BackendExportStatus, BackendMgmtRequest, BackendMgmtResponse, BackendRequest,
    BackendResponse,
};
use underwrap_protocol::policy::POLICY_DOCUMENT_SECRET_ID;
use underwrap_store::StorageBackend;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementConfig {
    pub socket: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    pub management: ManagementConfig,
    pub stores: HashMap<String, StorageConfig>,
}

struct ExportEntry {
    store: String,
    socket: PathBuf,
    mode: u32,
    abort: AbortHandle,
}

struct State {
    stores: HashMap<String, Arc<dyn StorageBackend>>,
    exports: Mutex<HashMap<PathBuf, ExportEntry>>,
}

pub async fn run(config: BackendConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    let stores = open_stores(&config.stores)?;
    let state = Arc::new(State {
        stores,
        exports: Mutex::new(HashMap::new()),
    });

    let management_listener = bind_socket(&config.management.socket, 0o600).with_context(|| {
        format!(
            "binding management socket {}",
            config.management.socket.display()
        )
    })?;
    info!(path = %config.management.socket.display(), "backend management socket listening");

    let mut tasks = JoinSet::new();
    tasks.spawn(management_accept_loop(
        management_listener,
        Arc::clone(&state),
    ));

    match shutdown {
        Some(notify) => {
            tokio::select! {
                r = tasks.join_next() => match r {
                    Some(result) => result.context("backend management task panicked")?,
                    None => Ok(()),
                },
                _ = notify.notified() => {
                    info!("backend shutdown signal received");
                    shutdown_exports(&state).await;
                    tasks.abort_all();
                    while tasks.join_next().await.is_some() {}
                    Ok(())
                }
            }
        }
        None => match tasks.join_next().await {
            Some(result) => result.context("backend management task panicked")?,
            None => Ok(()),
        },
    }
}

fn open_stores(
    configs: &HashMap<String, StorageConfig>,
) -> Result<HashMap<String, Arc<dyn StorageBackend>>> {
    let mut stores = HashMap::new();
    for (name, config) in configs {
        let store: Arc<dyn StorageBackend> = Arc::from(underwrap_store::open(config)?);
        stores.insert(name.clone(), store);
    }
    Ok(stores)
}

fn bind_socket(path: &Path, mode: u32) -> Result<UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating socket dir {}", parent.display()))?;
    }
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    }

    Ok(listener)
}

async fn management_accept_loop(listener: UnixListener, state: Arc<State>) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_management_connection(stream, &state).await {
                error!("backend management connection error: {e:#}");
            }
        });
    }
}

async fn handle_management_connection(mut stream: UnixStream, state: &State) -> Result<()> {
    loop {
        let request: BackendMgmtRequest = match read_frame_async(&mut stream).await {
            Ok(req) => req,
            Err(_) => break,
        };
        let response = process_management_request(request, state).await;
        write_frame_async(&mut stream, &response).await?;
    }
    Ok(())
}

async fn process_management_request(
    request: BackendMgmtRequest,
    state: &State,
) -> BackendMgmtResponse {
    match request {
        BackendMgmtRequest::ListStores => {
            let mut stores: Vec<String> = state.stores.keys().cloned().collect();
            stores.sort();
            BackendMgmtResponse::ListStoresOk { stores }
        }
        BackendMgmtRequest::ListExports => {
            let exports = state.exports.lock().await;
            let mut list: Vec<BackendExportStatus> = exports
                .values()
                .map(|entry| BackendExportStatus {
                    store: entry.store.clone(),
                    socket: entry.socket.display().to_string(),
                    mode: entry.mode,
                })
                .collect();
            list.sort_by(|a, b| a.socket.cmp(&b.socket));
            BackendMgmtResponse::ListExportsOk { exports: list }
        }
        BackendMgmtRequest::ExportSocket {
            store,
            socket,
            mode,
        } => match export_store_socket(state, &store, PathBuf::from(socket), mode).await {
            Ok(()) => BackendMgmtResponse::ExportSocketOk,
            Err(e) => BackendMgmtResponse::Error {
                code: BackendErrorCode::Internal,
                message: e.to_string(),
            },
        },
        BackendMgmtRequest::UnexportSocket { socket } => {
            let existed = unexport_store_socket(state, &PathBuf::from(socket)).await;
            BackendMgmtResponse::UnexportSocketOk { existed }
        }
        BackendMgmtRequest::GetPolicy { store } => match get_policy_document(state, &store) {
            Ok(document_json) => BackendMgmtResponse::GetPolicyOk { document_json },
            Err(e) => BackendMgmtResponse::Error {
                code: BackendErrorCode::Internal,
                message: e.to_string(),
            },
        },
        BackendMgmtRequest::PutPolicy {
            store,
            document_json,
        } => match put_policy_document(state, &store, &document_json) {
            Ok(()) => BackendMgmtResponse::PutPolicyOk,
            Err(e) => BackendMgmtResponse::Error {
                code: BackendErrorCode::Internal,
                message: e.to_string(),
            },
        },
    }
}

fn get_policy_document(state: &State, store_name: &str) -> Result<Option<String>> {
    let store = state
        .stores
        .get(store_name)
        .with_context(|| format!("unknown store '{store_name}'"))?;
    let document = store.read(POLICY_DOCUMENT_SECRET_ID)?;
    Ok(document.map(|bytes| String::from_utf8_lossy(bytes.as_ref()).into_owned()))
}

fn put_policy_document(state: &State, store_name: &str, document_json: &str) -> Result<()> {
    let store = state
        .stores
        .get(store_name)
        .with_context(|| format!("unknown store '{store_name}'"))?;
    // Validate that the JSON parses as a policy document before persisting it.
    let _: underwrap_protocol::policy::PolicyDocument =
        serde_json::from_str(document_json).context("parsing policy document json")?;
    store.write(POLICY_DOCUMENT_SECRET_ID, document_json.as_bytes())
}

async fn export_store_socket(
    state: &State,
    store_name: &str,
    socket: PathBuf,
    mode: u32,
) -> Result<()> {
    let store = state
        .stores
        .get(store_name)
        .with_context(|| format!("unknown store '{store_name}'"))?
        .clone();

    {
        let mut exports = state.exports.lock().await;
        if let Some(existing) = exports.remove(&socket) {
            existing.abort.abort();
        }
    }
    let _ = std::fs::remove_file(&socket);

    let listener = bind_socket(&socket, mode)
        .with_context(|| format!("binding export socket {}", socket.display()))?;
    info!(store = %store_name, path = %socket.display(), mode = format!("{mode:#o}"), "backend export socket listening");

    let store_name_owned = store_name.to_string();
    let socket_for_task = socket.clone();
    let handle = tokio::spawn(async move {
        if let Err(e) = data_accept_loop(listener, store).await {
            error!(store = %store_name_owned, path = %socket_for_task.display(), "backend data listener error: {e:#}");
        }
    });

    let mut exports = state.exports.lock().await;
    exports.insert(
        socket.clone(),
        ExportEntry {
            store: store_name.to_string(),
            socket,
            mode,
            abort: handle.abort_handle(),
        },
    );
    Ok(())
}

async fn unexport_store_socket(state: &State, socket: &Path) -> bool {
    let mut exports = state.exports.lock().await;
    if let Some(entry) = exports.remove(socket) {
        entry.abort.abort();
        let _ = std::fs::remove_file(socket);
        true
    } else {
        false
    }
}

async fn shutdown_exports(state: &State) {
    let sockets: Vec<PathBuf> = {
        let exports = state.exports.lock().await;
        exports.keys().cloned().collect()
    };
    for socket in sockets {
        let _ = unexport_store_socket(state, &socket).await;
    }
}

async fn data_accept_loop(listener: UnixListener, store: Arc<dyn StorageBackend>) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let store = Arc::clone(&store);
        tokio::spawn(async move {
            if let Err(e) = handle_data_connection(stream, &*store).await {
                error!("backend data connection error: {e:#}");
            }
        });
    }
}

async fn handle_data_connection(mut stream: UnixStream, store: &dyn StorageBackend) -> Result<()> {
    loop {
        let request: BackendRequest = match read_frame_async(&mut stream).await {
            Ok(req) => req,
            Err(_) => break,
        };
        let response = process_request(&request, store).await;
        write_frame_async(&mut stream, &response).await?;
    }
    Ok(())
}

async fn process_request(request: &BackendRequest, store: &dyn StorageBackend) -> BackendResponse {
    match request {
        BackendRequest::Read { secret_id } => match store.read(secret_id) {
            Ok(material) => {
                info!(secret = %secret_id, present = material.is_some(), "backend read");
                BackendResponse::ReadOk { material }
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },
        BackendRequest::Write {
            secret_id,
            material,
        } => match store.write(secret_id, material.as_ref()) {
            Ok(()) => {
                info!(secret = %secret_id, "backend write");
                BackendResponse::WriteOk
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },
        BackendRequest::List { prefix } => match store.list(prefix) {
            Ok(ids) => {
                info!(prefix = %prefix, count = ids.len(), "backend list");
                BackendResponse::ListOk { ids }
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },
        BackendRequest::Delete { secret_id } => match store.delete(secret_id) {
            Ok(existed) => {
                info!(secret = %secret_id, existed = existed, "backend delete");
                BackendResponse::DeleteOk { existed }
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },
        BackendRequest::Status => {
            info!("backend status");
            BackendResponse::StatusOk
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;
    use underwrap_protocol::message::SecretBytes;

    struct TestStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
        fail_ops: Mutex<HashSet<&'static str>>,
    }

    impl TestStore {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
                fail_ops: Mutex::new(HashSet::new()),
            }
        }

        fn with_data(entries: &[(&str, &[u8])]) -> Self {
            let store = Self::new();
            let mut data = store.data.lock().unwrap();
            for (key, value) in entries {
                data.insert((*key).to_string(), value.to_vec());
            }
            drop(data);
            store
        }

        fn fail(&self, op: &'static str) {
            self.fail_ops.lock().unwrap().insert(op);
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

    #[tokio::test]
    async fn process_request_read_write_list_delete_and_status() {
        let store = TestStore::with_data(&[("github/token", b"ghp_test")]);

        let resp = process_request(
            &BackendRequest::Read {
                secret_id: "github/token".to_string(),
            },
            &store,
        )
        .await;
        assert_eq!(
            resp,
            BackendResponse::ReadOk {
                material: Some(b"ghp_test".as_slice().into())
            }
        );

        let resp = process_request(
            &BackendRequest::Write {
                secret_id: "github/user".to_string(),
                material: b"octocat".as_slice().into(),
            },
            &store,
        )
        .await;
        assert_eq!(resp, BackendResponse::WriteOk);

        let resp = process_request(
            &BackendRequest::List {
                prefix: "github".to_string(),
            },
            &store,
        )
        .await;
        assert_eq!(
            resp,
            BackendResponse::ListOk {
                ids: vec!["github/token".to_string(), "github/user".to_string()]
            }
        );

        let resp = process_request(
            &BackendRequest::Delete {
                secret_id: "github/token".to_string(),
            },
            &store,
        )
        .await;
        assert_eq!(resp, BackendResponse::DeleteOk { existed: true });

        let resp = process_request(&BackendRequest::Status, &store).await;
        assert_eq!(resp, BackendResponse::StatusOk);
    }

    #[tokio::test]
    async fn process_request_maps_storage_errors() {
        for (op, request) in [
            (
                "read",
                BackendRequest::Read {
                    secret_id: "github/token".to_string(),
                },
            ),
            (
                "write",
                BackendRequest::Write {
                    secret_id: "github/token".to_string(),
                    material: b"value".as_slice().into(),
                },
            ),
            (
                "list",
                BackendRequest::List {
                    prefix: "github".to_string(),
                },
            ),
            (
                "delete",
                BackendRequest::Delete {
                    secret_id: "github/token".to_string(),
                },
            ),
        ] {
            let store = TestStore::new();
            store.fail(op);

            let resp = process_request(&request, &store).await;
            match resp {
                BackendResponse::Error { code, message } => {
                    assert_eq!(code, BackendErrorCode::Internal);
                    assert!(message.contains("storage error"));
                    assert!(message.contains(op));
                }
                other => panic!("expected Error, got {other:?}"),
            }
        }
    }

    #[test]
    fn policy_document_roundtrips_through_management_helpers() {
        let state = State {
            stores: HashMap::from([(
                "work".to_string(),
                Arc::new(TestStore::new()) as Arc<dyn StorageBackend>,
            )]),
            exports: tokio::sync::Mutex::new(HashMap::new()),
        };

        let json = r#"{"wrappers":[{"wrapper":"gh-comment","enabled":true}],"rules":[{"id":"r1","wrapper":"gh-comment","resource":"secret:github/token","operation":"secret.read","effect":"allow"}]}"#;
        put_policy_document(&state, "work", json).unwrap();
        let stored = get_policy_document(&state, "work").unwrap();
        assert_eq!(stored.as_deref(), Some(json));
    }

    #[test]
    fn put_policy_document_rejects_invalid_json() {
        let state = State {
            stores: HashMap::from([(
                "work".to_string(),
                Arc::new(TestStore::new()) as Arc<dyn StorageBackend>,
            )]),
            exports: tokio::sync::Mutex::new(HashMap::new()),
        };

        let err = put_policy_document(&state, "work", "not-json").unwrap_err();
        assert!(err.to_string().contains("parsing policy document json"));
    }
}
