//! Backend daemon server.
//!
//! Listens on a Unix socket, handles BackendRequest messages,
//! serves secret material from a StorageBackend.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;
use tracing::{error, info};

use underwrap_protocol::config::StorageConfig;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{BackendErrorCode, BackendRequest, BackendResponse};
use underwrap_store::StorageBackend;

/// One frontend/backend binding served by this backend daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendBindingConfig {
    /// Path to the Unix socket to listen on for this binding.
    pub socket: PathBuf,
    /// Unix mode for the socket file.
    #[serde(default = "default_binding_socket_mode")]
    pub mode: u32,
    /// Optional storage prefix used to namespace all keys for this binding.
    /// If omitted, the binding sees the storage root directly.
    #[serde(default)]
    pub prefix: Option<String>,
}

fn default_binding_socket_mode() -> u32 {
    0o600
}

/// Backend daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Storage backend configuration owned by this backend daemon.
    #[serde(default)]
    pub storage: StorageConfig,

    /// Named frontend bindings served by this daemon.
    pub bindings: std::collections::HashMap<String, BackendBindingConfig>,
}

struct State {
    store: Arc<dyn StorageBackend>,
}

struct ScopedStore {
    prefix: Option<String>,
    inner: Arc<dyn StorageBackend>,
}

impl ScopedStore {
    fn scoped_id(&self, secret_id: &str) -> String {
        match &self.prefix {
            Some(prefix) => format!("{}/{}", prefix, secret_id),
            None => secret_id.to_string(),
        }
    }

    fn scoped_prefix(&self, prefix: &str) -> String {
        match &self.prefix {
            Some(binding_prefix) => {
                if prefix.is_empty() {
                    binding_prefix.clone()
                } else {
                    format!("{}/{}", binding_prefix, prefix)
                }
            }
            None => prefix.to_string(),
        }
    }
}

impl StorageBackend for ScopedStore {
    fn read(
        &self,
        secret_id: &str,
    ) -> anyhow::Result<Option<underwrap_protocol::message::SecretBytes>> {
        self.inner.read(&self.scoped_id(secret_id))
    }

    fn write(&self, secret_id: &str, material: &[u8]) -> anyhow::Result<()> {
        self.inner.write(&self.scoped_id(secret_id), material)
    }

    fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        let scoped_prefix = self.scoped_prefix(prefix);
        let ids = self.inner.list(&scoped_prefix)?;
        match &self.prefix {
            Some(binding_prefix) => {
                let strip_prefix = format!("{binding_prefix}/");
                Ok(ids
                    .into_iter()
                    .filter_map(|id| id.strip_prefix(&strip_prefix).map(|s| s.to_string()))
                    .collect())
            }
            None => Ok(ids),
        }
    }

    fn delete(&self, secret_id: &str) -> anyhow::Result<bool> {
        self.inner.delete(&self.scoped_id(secret_id))
    }
}

pub async fn run(config: BackendConfig, shutdown: Option<Arc<Notify>>) -> Result<()> {
    let store: Arc<dyn StorageBackend> = Arc::from(underwrap_store::open(&config.storage)?);
    let mut listeners = Vec::new();

    for (binding, binding_config) in &config.bindings {
        if let Some(parent) = binding_config.socket.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating socket dir {}", parent.display()))?;
        }
        let _ = std::fs::remove_file(&binding_config.socket);

        let listener = UnixListener::bind(&binding_config.socket).with_context(|| {
            format!("binding backend socket {}", binding_config.socket.display())
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                &binding_config.socket,
                std::fs::Permissions::from_mode(binding_config.mode),
            )?;
        }

        info!(binding = %binding, path = %binding_config.socket.display(), "backend socket listening");
        listeners.push((binding.clone(), binding_config.prefix.clone(), listener));
    }

    let mut tasks = tokio::task::JoinSet::new();
    for (_binding, prefix, listener) in listeners {
        let scoped_store: Arc<dyn StorageBackend> = Arc::new(ScopedStore {
            prefix,
            inner: Arc::clone(&store),
        });
        let state = Arc::new(State {
            store: scoped_store,
        });
        tasks.spawn(async move { accept_loop(listener, state).await });
    }

    match shutdown {
        Some(notify) => {
            tokio::select! {
                r = tasks.join_next() => match r {
                    Some(result) => result.context("backend listener task panicked")?,
                    None => Ok(()),
                },
                _ = notify.notified() => {
                    info!("backend shutdown signal received");
                    tasks.abort_all();
                    while tasks.join_next().await.is_some() {}
                    Ok(())
                }
            }
        }
        None => match tasks.join_next().await {
            Some(result) => result.context("backend listener task panicked")?,
            None => Ok(()),
        },
    }
}

async fn accept_loop(listener: UnixListener, state: Arc<State>) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, &state).await {
                error!("backend connection error: {e:#}");
            }
        });
    }
}

async fn handle_connection(mut stream: UnixStream, state: &State) -> Result<()> {
    loop {
        let request: BackendRequest = match read_frame_async(&mut stream).await {
            Ok(req) => req,
            Err(_) => break,
        };

        let response = process_request(&request, state).await;
        write_frame_async(&mut stream, &response).await?;
    }
    Ok(())
}

async fn process_request(request: &BackendRequest, state: &State) -> BackendResponse {
    match request {
        BackendRequest::Read { secret_id } => match state.store.read(secret_id) {
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
        } => match state.store.write(secret_id, material.as_ref()) {
            Ok(()) => {
                info!(secret = %secret_id, "backend write");
                BackendResponse::WriteOk
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },

        BackendRequest::List { prefix } => match state.store.list(prefix) {
            Ok(ids) => {
                info!(prefix = %prefix, count = ids.len(), "backend list");
                BackendResponse::ListOk { ids }
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },

        BackendRequest::Delete { secret_id } => match state.store.delete(secret_id) {
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

    fn state_with_store(store: TestStore) -> State {
        State {
            store: Arc::new(store),
        }
    }

    #[tokio::test]
    async fn process_request_read_write_list_delete_and_status() {
        let state = state_with_store(TestStore::with_data(&[("github/token", b"ghp_test")]));

        let resp = process_request(
            &BackendRequest::Read {
                secret_id: "github/token".to_string(),
            },
            &state,
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
            &state,
        )
        .await;
        assert_eq!(resp, BackendResponse::WriteOk);

        let resp = process_request(
            &BackendRequest::List {
                prefix: "github".to_string(),
            },
            &state,
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
            &state,
        )
        .await;
        assert_eq!(resp, BackendResponse::DeleteOk { existed: true });

        let resp = process_request(&BackendRequest::Status, &state).await;
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
            let state = state_with_store(store);

            let resp = process_request(&request, &state).await;
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
}
