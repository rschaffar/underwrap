//! Remote storage backend — connects to underwrap-backendd over a Unix socket.

use anyhow::{Context, Result, bail};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{BackendErrorCode, BackendRequest, BackendResponse};

use crate::StorageBackend;

/// A storage backend that delegates to a remote underwrap-backendd
/// over a Unix socket (typically SSH-tunneled).
pub struct RemoteBackend {
    socket_path: PathBuf,
    /// Persistent connection, reconnects on failure.
    conn: Mutex<Option<UnixStream>>,
}

impl RemoteBackend {
    pub fn open(socket_path: &Path) -> Result<Self> {
        Ok(Self {
            socket_path: socket_path.to_path_buf(),
            conn: Mutex::new(None),
        })
    }

    fn send_request(&self, request: &BackendRequest) -> Result<BackendResponse> {
        let mut conn_guard = self.conn.lock().unwrap();

        // Try existing connection first, reconnect on failure.
        if let Some(ref mut stream) = *conn_guard {
            match self.try_request(stream, request) {
                Ok(resp) => return Ok(resp),
                Err(_) => {
                    // Connection dead, will reconnect below.
                    *conn_guard = None;
                }
            }
        }

        // Connect (or reconnect).
        let mut stream = UnixStream::connect(&self.socket_path)
            .with_context(|| format!("connecting to backend at {}", self.socket_path.display()))?;

        let resp = self.try_request(&mut stream, request)?;
        *conn_guard = Some(stream);
        Ok(resp)
    }

    fn try_request(
        &self,
        stream: &mut UnixStream,
        request: &BackendRequest,
    ) -> Result<BackendResponse> {
        write_frame_sync(stream, request)?;
        let resp: BackendResponse = read_frame_sync(stream)?;
        Ok(resp)
    }
}

impl StorageBackend for RemoteBackend {
    fn read(&self, secret_id: &str) -> Result<Option<String>> {
        let resp = self.send_request(&BackendRequest::Read {
            secret_id: secret_id.to_string(),
        })?;

        match resp {
            BackendResponse::Ok { material, .. } => Ok(material.map(Into::into)),
            BackendResponse::Error { code, message } => match code {
                BackendErrorCode::NotFound => Ok(None),
                _ => bail!("backend error ({code:?}): {message}"),
            },
        }
    }

    fn write(&self, secret_id: &str, material: &str) -> Result<()> {
        let resp = self.send_request(&BackendRequest::Write {
            secret_id: secret_id.to_string(),
            material: material.to_string().into(),
        })?;

        match resp {
            BackendResponse::Ok { .. } => Ok(()),
            BackendResponse::Error { code, message } => {
                bail!("backend error ({code:?}): {message}")
            }
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let resp = self.send_request(&BackendRequest::List {
            prefix: prefix.to_string(),
        })?;

        match resp {
            BackendResponse::Ok {
                info: Some(info), ..
            } => {
                let ids: Vec<String> =
                    serde_json::from_value(info).context("parsing list response")?;
                Ok(ids)
            }
            BackendResponse::Ok { info: None, .. } => Ok(Vec::new()),
            BackendResponse::Error { code, message } => {
                bail!("backend list error ({code:?}): {message}")
            }
        }
    }

    fn delete(&self, secret_id: &str) -> Result<bool> {
        let resp = self.send_request(&BackendRequest::Delete {
            secret_id: secret_id.to_string(),
        })?;

        match resp {
            BackendResponse::Ok {
                info: Some(info), ..
            } => Ok(info.as_bool().unwrap_or(false)),
            BackendResponse::Ok { info: None, .. } => Ok(true),
            BackendResponse::Error { code, message } => {
                bail!("backend delete error ({code:?}): {message}")
            }
        }
    }
}

impl RemoteBackend {
    /// Read with derivation (e.g. OAuth refresh → access token).
    pub fn derive(&self, secret_id: &str, mode: &str) -> Result<Option<String>> {
        let resp = self.send_request(&BackendRequest::Derive {
            secret_id: secret_id.to_string(),
            mode: mode.to_string(),
        })?;

        match resp {
            BackendResponse::Ok { material, .. } => Ok(material.map(Into::into)),
            BackendResponse::Error { code, message } => match code {
                BackendErrorCode::NotFound => Ok(None),
                BackendErrorCode::DerivationFailed => bail!("backend derive failed: {message}"),
                _ => bail!("backend derive error ({code:?}): {message}"),
            },
        }
    }

    /// Check if the backend is reachable.
    pub fn is_available(&self) -> bool {
        self.send_request(&BackendRequest::Status).is_ok()
    }
}
