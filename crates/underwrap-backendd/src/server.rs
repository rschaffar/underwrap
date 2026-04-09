//! Backend daemon server.
//!
//! Listens on a Unix socket, handles BackendRequest messages,
//! serves secret material from a StorageBackend.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tracing::{error, info};

use underwrap_protocol::config::StorageConfig;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{BackendErrorCode, BackendRequest, BackendResponse};
use underwrap_store::StorageBackend;

/// Backend daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Path to the Unix socket to listen on.
    pub socket: PathBuf,

    /// Storage backend configuration.
    #[serde(default)]
    pub storage: StorageConfig,
}

struct State {
    store: Box<dyn StorageBackend>,
}

pub async fn run(config: BackendConfig) -> Result<()> {
    let store = underwrap_store::open(&config.storage)?;
    let state = Arc::new(State { store });

    if let Some(parent) = config.socket.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating socket dir {}", parent.display()))?;
    }
    let _ = std::fs::remove_file(&config.socket);

    let listener = UnixListener::bind(&config.socket)
        .with_context(|| format!("binding backend socket {}", config.socket.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config.socket, std::fs::Permissions::from_mode(0o600))?;
    }

    info!(path = %config.socket.display(), "backend socket listening");

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
            store: Box::new(store),
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
