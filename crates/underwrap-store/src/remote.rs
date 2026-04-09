//! Remote storage backend — connects to underwrap-backendd over a Unix socket.

use anyhow::{Context, Result, bail};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{BackendErrorCode, BackendRequest, BackendResponse, SecretBytes};

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

        if let Some(ref mut stream) = *conn_guard {
            match self.try_request(stream, request) {
                Ok(resp) => return Ok(resp),
                Err(_) => {
                    *conn_guard = None;
                }
            }
        }

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
    fn read(&self, secret_id: &str) -> Result<Option<SecretBytes>> {
        let resp = self.send_request(&BackendRequest::Read {
            secret_id: secret_id.to_string(),
        })?;

        match resp {
            BackendResponse::ReadOk { material } => Ok(material),
            BackendResponse::Error { code, message } => match code {
                BackendErrorCode::NotFound => Ok(None),
                _ => bail!("backend error ({code:?}): {message}"),
            },
            other => bail!("unexpected backend response to read: {other:?}"),
        }
    }

    fn write(&self, secret_id: &str, material: &[u8]) -> Result<()> {
        let resp = self.send_request(&BackendRequest::Write {
            secret_id: secret_id.to_string(),
            material: material.into(),
        })?;

        match resp {
            BackendResponse::WriteOk => Ok(()),
            BackendResponse::Error { code, message } => {
                bail!("backend error ({code:?}): {message}")
            }
            other => bail!("unexpected backend response to write: {other:?}"),
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let resp = self.send_request(&BackendRequest::List {
            prefix: prefix.to_string(),
        })?;

        match resp {
            BackendResponse::ListOk { ids } => Ok(ids),
            BackendResponse::Error { code, message } => {
                bail!("backend list error ({code:?}): {message}")
            }
            other => bail!("unexpected backend response to list: {other:?}"),
        }
    }

    fn delete(&self, secret_id: &str) -> Result<bool> {
        let resp = self.send_request(&BackendRequest::Delete {
            secret_id: secret_id.to_string(),
        })?;

        match resp {
            BackendResponse::DeleteOk { existed } => Ok(existed),
            BackendResponse::Error { code, message } => {
                bail!("backend delete error ({code:?}): {message}")
            }
            other => bail!("unexpected backend response to delete: {other:?}"),
        }
    }
}

impl RemoteBackend {
    /// Check if the backend is reachable.
    pub fn is_available(&self) -> bool {
        matches!(
            self.send_request(&BackendRequest::Status),
            Ok(BackendResponse::StatusOk)
        )
    }
}
