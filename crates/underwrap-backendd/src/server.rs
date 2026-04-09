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
