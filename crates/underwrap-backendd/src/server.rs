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

use crate::derive;

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

    // Set socket permissions to 0600 — only the daemon user should access it.
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
    // Backend connections are persistent — handle multiple requests per connection.
    loop {
        let request: BackendRequest = match read_frame_async(&mut stream).await {
            Ok(req) => req,
            Err(_) => break, // Connection closed.
        };

        let response = process_request(&request, state).await;
        write_frame_async(&mut stream, &response).await?;
    }
    Ok(())
}

async fn process_request(request: &BackendRequest, state: &State) -> BackendResponse {
    match request {
        BackendRequest::Read { secret_id } => match state.store.read(secret_id) {
            Ok(Some(material)) => match material.into_utf8_string() {
                Ok(material) => {
                    info!(secret = %secret_id, "backend read");
                    BackendResponse::Ok {
                        material: Some(material.into()),
                        info: None,
                    }
                }
                Err(e) => BackendResponse::Error {
                    code: BackendErrorCode::Internal,
                    message: format!("stored secret is not valid UTF-8: {e}"),
                },
            },
            Ok(None) => BackendResponse::Error {
                code: BackendErrorCode::NotFound,
                message: format!("secret '{secret_id}' not found"),
            },
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },

        BackendRequest::Derive { secret_id, mode } => {
            // Read the raw material, then apply derivation.
            let material = match state.store.read(secret_id) {
                Ok(Some(m)) => m,
                Ok(None) => {
                    return BackendResponse::Error {
                        code: BackendErrorCode::NotFound,
                        message: format!("secret '{secret_id}' not found"),
                    };
                }
                Err(e) => {
                    return BackendResponse::Error {
                        code: BackendErrorCode::Internal,
                        message: format!("storage error: {e}"),
                    };
                }
            };

            match derive::derive(mode, secret_id, material.as_ref()).await {
                Ok(derived) => {
                    info!(secret = %secret_id, mode = %mode, "backend derive");
                    BackendResponse::Ok {
                        material: Some(derived.into()),
                        info: None,
                    }
                }
                Err(e) => BackendResponse::Error {
                    code: BackendErrorCode::DerivationFailed,
                    message: format!("derivation failed: {e}"),
                },
            }
        }

        BackendRequest::Write {
            secret_id,
            material,
        } => match state.store.write(secret_id, material.as_bytes()) {
            Ok(()) => {
                info!(secret = %secret_id, "backend write");
                BackendResponse::Ok {
                    material: None,
                    info: None,
                }
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },

        BackendRequest::List { prefix } => match state.store.list(prefix) {
            Ok(ids) => {
                info!(prefix = %prefix, count = ids.len(), "backend list");
                BackendResponse::Ok {
                    material: None,
                    info: Some(serde_json::json!(ids)),
                }
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },

        BackendRequest::Delete { secret_id } => match state.store.delete(secret_id) {
            Ok(existed) => {
                info!(secret = %secret_id, existed = existed, "backend delete");
                BackendResponse::Ok {
                    material: None,
                    info: Some(serde_json::json!(existed)),
                }
            }
            Err(e) => BackendResponse::Error {
                code: BackendErrorCode::Internal,
                message: format!("storage error: {e}"),
            },
        },

        BackendRequest::Status => {
            info!("backend status");
            BackendResponse::Ok {
                material: None,
                info: Some(serde_json::json!({"status": "ok"})),
            }
        }
    }
}
