//! Remote storage backend — connects to underwrap-backendd over a Unix socket.

use anyhow::{Context, Result, bail};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use underwrap_protocol::ipc::{read_frame_sync, write_frame_sync};
use underwrap_protocol::message::{BackendRequest, BackendResponse, SecretBytes};

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
            BackendResponse::Error { code, message } => {
                bail!("backend error ({code:?}): {message}")
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    use underwrap_protocol::message::BackendErrorCode;

    fn temp_socket_path(name: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(format!("{name}.sock"));
        (dir, path)
    }

    fn spawn_server<F>(socket_path: PathBuf, handler: F) -> thread::JoinHandle<()>
    where
        F: FnOnce(UnixListener) + Send + 'static,
    {
        thread::spawn(move || {
            let listener = UnixListener::bind(&socket_path).unwrap();
            handler(listener);
        })
    }

    fn wait_for_socket(path: &Path) {
        for _ in 0..100 {
            if path.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for test socket at {}", path.display());
    }

    #[test]
    fn read_write_list_delete_and_status_roundtrip() {
        let (_dir, socket_path) = temp_socket_path("remote-roundtrip");
        let handle = spawn_server(socket_path.clone(), |listener| {
            let (mut stream, _) = listener.accept().unwrap();

            let req: BackendRequest = read_frame_sync(&mut stream).unwrap();
            assert_eq!(
                req,
                BackendRequest::Read {
                    secret_id: "github/token".to_string()
                }
            );
            write_frame_sync(
                &mut stream,
                &BackendResponse::ReadOk {
                    material: Some(b"ghp_test".as_slice().into()),
                },
            )
            .unwrap();

            let req: BackendRequest = read_frame_sync(&mut stream).unwrap();
            assert_eq!(
                req,
                BackendRequest::Write {
                    secret_id: "github/token".to_string(),
                    material: b"updated".as_slice().into(),
                }
            );
            write_frame_sync(&mut stream, &BackendResponse::WriteOk).unwrap();

            let req: BackendRequest = read_frame_sync(&mut stream).unwrap();
            assert_eq!(
                req,
                BackendRequest::List {
                    prefix: "github".to_string()
                }
            );
            write_frame_sync(
                &mut stream,
                &BackendResponse::ListOk {
                    ids: vec!["github/token".to_string(), "github/user".to_string()],
                },
            )
            .unwrap();

            let req: BackendRequest = read_frame_sync(&mut stream).unwrap();
            assert_eq!(
                req,
                BackendRequest::Delete {
                    secret_id: "github/token".to_string()
                }
            );
            write_frame_sync(&mut stream, &BackendResponse::DeleteOk { existed: true }).unwrap();

            let req: BackendRequest = read_frame_sync(&mut stream).unwrap();
            assert_eq!(req, BackendRequest::Status);
            write_frame_sync(&mut stream, &BackendResponse::StatusOk).unwrap();
        });

        wait_for_socket(&socket_path);
        let backend = RemoteBackend::open(&socket_path).unwrap();
        assert_eq!(backend.read("github/token").unwrap().unwrap(), b"ghp_test");
        backend.write("github/token", b"updated").unwrap();
        assert_eq!(
            backend.list("github").unwrap(),
            vec!["github/token".to_string(), "github/user".to_string()]
        );
        assert!(backend.delete("github/token").unwrap());
        assert!(backend.is_available());

        handle.join().unwrap();
    }

    #[test]
    fn backend_errors_are_mapped_to_client_errors() {
        let (_dir, socket_path) = temp_socket_path("remote-errors");
        let handle = spawn_server(socket_path.clone(), |listener| {
            let (mut stream, _) = listener.accept().unwrap();
            let req: BackendRequest = read_frame_sync(&mut stream).unwrap();
            assert_eq!(
                req,
                BackendRequest::Read {
                    secret_id: "github/token".to_string()
                }
            );
            write_frame_sync(
                &mut stream,
                &BackendResponse::Error {
                    code: BackendErrorCode::Internal,
                    message: "boom".to_string(),
                },
            )
            .unwrap();
        });

        wait_for_socket(&socket_path);
        let backend = RemoteBackend::open(&socket_path).unwrap();
        let err = backend.read("github/token").unwrap_err();
        assert!(err.to_string().contains("backend error"));
        assert!(err.to_string().contains("boom"));

        handle.join().unwrap();
    }

    #[test]
    fn unexpected_response_is_rejected() {
        let (_dir, socket_path) = temp_socket_path("remote-unexpected");
        let handle = spawn_server(socket_path.clone(), |listener| {
            let (mut stream, _) = listener.accept().unwrap();
            let req: BackendRequest = read_frame_sync(&mut stream).unwrap();
            assert_eq!(
                req,
                BackendRequest::Write {
                    secret_id: "github/token".to_string(),
                    material: b"value".as_slice().into(),
                }
            );
            write_frame_sync(&mut stream, &BackendResponse::StatusOk).unwrap();
        });

        wait_for_socket(&socket_path);
        let backend = RemoteBackend::open(&socket_path).unwrap();
        let err = backend.write("github/token", b"value").unwrap_err();
        assert!(err.to_string().contains("unexpected backend response to write"));

        handle.join().unwrap();
    }

    #[test]
    fn reconnects_after_dropped_connection() {
        let (_dir, socket_path) = temp_socket_path("remote-reconnect");
        let connections = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&connections);
        let handle = spawn_server(socket_path.clone(), move |listener| {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                seen.fetch_add(1, Ordering::SeqCst);
                let req: BackendRequest = read_frame_sync(&mut stream).unwrap();
                assert_eq!(
                    req,
                    BackendRequest::Read {
                        secret_id: "github/token".to_string()
                    }
                );
                write_frame_sync(
                    &mut stream,
                    &BackendResponse::ReadOk {
                        material: Some(b"ghp_test".as_slice().into()),
                    },
                )
                .unwrap();
                drop(stream);
            }
        });

        wait_for_socket(&socket_path);
        let backend = RemoteBackend::open(&socket_path).unwrap();
        assert_eq!(backend.read("github/token").unwrap().unwrap(), b"ghp_test");
        assert_eq!(backend.read("github/token").unwrap().unwrap(), b"ghp_test");
        assert_eq!(connections.load(Ordering::SeqCst), 2);

        handle.join().unwrap();
    }
}
