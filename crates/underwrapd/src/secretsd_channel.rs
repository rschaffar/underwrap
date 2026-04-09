//! Secretsd channel handler — serves scoped StorageBackend operations
//! over an authenticated socketpair.
//!
//! The daemon creates this handler when a libsecret-enabled wrapper
//! connects. The handler automatically prefixes all secret IDs with
//! the wrapper's namespace (`<wrapper>/libsecret/`), ensuring the
//! secretsd process can only access its own secrets.

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::net::UnixStream;
use tracing::{debug, error, info, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{SecretsdErrorCode, SecretsdRequest, SecretsdResponse};
use underwrap_store::StorageBackend;

/// Run the secretsd channel handler loop on the daemon's end of the socketpair.
///
/// All operations are scoped to `{prefix}` which is set to
/// `<wrapper-name>/libsecret/` at creation time. The secretsd process
/// sends relative IDs; this handler prepends the prefix before calling
/// the StorageBackend.
///
/// Returns when the channel is closed (secretsd exited / namespace died).
pub async fn run_channel(
    stream: UnixStream,
    store: Arc<dyn StorageBackend>,
    prefix: String,
    wrapper_name: String,
    access: AccessMode,
) {
    info!(wrapper = %wrapper_name, prefix = %prefix, "secretsd channel opened");

    if let Err(e) = channel_loop(stream, store, &prefix, &wrapper_name, access).await {
        // EOF is normal (namespace died), other errors are logged.
        debug!(wrapper = %wrapper_name, err = %e, "secretsd channel closed");
    }

    info!(wrapper = %wrapper_name, "secretsd channel handler exited");
}

async fn channel_loop(
    mut stream: UnixStream,
    store: Arc<dyn StorageBackend>,
    prefix: &str,
    wrapper_name: &str,
    access: AccessMode,
) -> Result<()> {
    loop {
        let request: SecretsdRequest = match read_frame_async(&mut stream).await {
            Ok(req) => req,
            Err(e) => {
                // Check if it's just EOF (clean shutdown).
                let msg = format!("{e}");
                if msg.contains("unexpected eof")
                    || msg.contains("connection reset")
                    || msg.contains("broken pipe")
                {
                    return Ok(());
                }
                return Err(e).context("reading secretsd request");
            }
        };

        let response = process_request(&request, &*store, prefix, wrapper_name, access);
        write_frame_async(&mut stream, &response).await?;
    }
}

fn process_request(
    request: &SecretsdRequest,
    store: &dyn StorageBackend,
    prefix: &str,
    wrapper_name: &str,
    access: AccessMode,
) -> SecretsdResponse {
    match request {
        SecretsdRequest::Read { secret_id } => {
            let full_id = scoped_id(prefix, secret_id);
            if full_id.is_none() {
                warn!(wrapper = %wrapper_name, id = %secret_id, "rejected invalid secretsd read");
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::InvalidRequest,
                    message: "invalid secret id".to_string(),
                };
            }
            let full_id = full_id.unwrap();

            match store.read(&full_id) {
                Ok(material) => {
                    debug!(wrapper = %wrapper_name, id = %full_id, "secretsd read");
                    crate::audit::secretsd_read(wrapper_name, secret_id);
                    let material = match material {
                        Some(material) => match material.into_utf8_string() {
                            Ok(s) => Some(s.into()),
                            Err(e) => {
                                return SecretsdResponse::Error {
                                    code: SecretsdErrorCode::Internal,
                                    message: format!("stored secret is not valid UTF-8: {e}"),
                                };
                            }
                        },
                        None => None,
                    };
                    SecretsdResponse::Ok {
                        material,
                        ids: None,
                        existed: None,
                    }
                }
                Err(e) => {
                    error!(wrapper = %wrapper_name, id = %full_id, err = %e, "secretsd read error");
                    SecretsdResponse::Error {
                        code: SecretsdErrorCode::Internal,
                        message: format!("storage error: {e}"),
                    }
                }
            }
        }

        SecretsdRequest::Write {
            secret_id,
            material,
        } => {
            if access != AccessMode::ReadWrite {
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::Unauthorized,
                    message: "credential is read-only".to_string(),
                };
            }
            let full_id = scoped_id(prefix, secret_id);
            if full_id.is_none() {
                warn!(wrapper = %wrapper_name, id = %secret_id, "rejected invalid secretsd write");
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::InvalidRequest,
                    message: "invalid secret id".to_string(),
                };
            }
            let full_id = full_id.unwrap();

            match store.write(&full_id, material.as_bytes()) {
                Ok(()) => {
                    debug!(wrapper = %wrapper_name, id = %full_id, "secretsd write");
                    crate::audit::secretsd_write(wrapper_name, secret_id);
                    SecretsdResponse::Ok {
                        material: None,
                        ids: None,
                        existed: None,
                    }
                }
                Err(e) => {
                    error!(wrapper = %wrapper_name, id = %full_id, err = %e, "secretsd write error");
                    SecretsdResponse::Error {
                        code: SecretsdErrorCode::Internal,
                        message: format!("storage error: {e}"),
                    }
                }
            }
        }

        SecretsdRequest::List { prefix: rel_prefix } => {
            let full_prefix = if rel_prefix.is_empty() {
                prefix.to_string()
            } else {
                // Validate the relative prefix.
                if has_traversal(rel_prefix) {
                    warn!(wrapper = %wrapper_name, prefix = %rel_prefix, "rejected invalid secretsd list prefix");
                    return SecretsdResponse::Error {
                        code: SecretsdErrorCode::InvalidRequest,
                        message: "invalid prefix".to_string(),
                    };
                }
                format!("{prefix}{rel_prefix}")
            };

            match store.list(&full_prefix) {
                Ok(ids) => {
                    // Strip the prefix from returned IDs so secretsd sees relative paths.
                    let relative_ids: Vec<String> = ids
                        .into_iter()
                        .filter_map(|id| id.strip_prefix(prefix).map(|s| s.to_string()))
                        .collect();
                    debug!(wrapper = %wrapper_name, prefix = %full_prefix, count = relative_ids.len(), "secretsd list");
                    SecretsdResponse::Ok {
                        material: None,
                        ids: Some(relative_ids),
                        existed: None,
                    }
                }
                Err(e) => {
                    error!(wrapper = %wrapper_name, prefix = %full_prefix, err = %e, "secretsd list error");
                    SecretsdResponse::Error {
                        code: SecretsdErrorCode::Internal,
                        message: format!("storage error: {e}"),
                    }
                }
            }
        }

        SecretsdRequest::Delete { secret_id } => {
            if access != AccessMode::ReadWrite {
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::Unauthorized,
                    message: "credential is read-only".to_string(),
                };
            }
            let full_id = scoped_id(prefix, secret_id);
            if full_id.is_none() {
                warn!(wrapper = %wrapper_name, id = %secret_id, "rejected invalid secretsd delete");
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::InvalidRequest,
                    message: "invalid secret id".to_string(),
                };
            }
            let full_id = full_id.unwrap();

            match store.delete(&full_id) {
                Ok(existed) => {
                    debug!(wrapper = %wrapper_name, id = %full_id, existed = existed, "secretsd delete");
                    crate::audit::secretsd_delete(wrapper_name, secret_id);
                    SecretsdResponse::Ok {
                        material: None,
                        ids: None,
                        existed: Some(existed),
                    }
                }
                Err(e) => {
                    error!(wrapper = %wrapper_name, id = %full_id, err = %e, "secretsd delete error");
                    SecretsdResponse::Error {
                        code: SecretsdErrorCode::Internal,
                        message: format!("storage error: {e}"),
                    }
                }
            }
        }
    }
}

/// Construct a scoped secret ID by prepending the prefix.
/// Returns None if the relative ID contains traversal components.
fn scoped_id(prefix: &str, relative_id: &str) -> Option<String> {
    if relative_id.is_empty() || has_traversal(relative_id) {
        return None;
    }
    Some(format!("{prefix}{relative_id}"))
}

/// Check for path traversal attempts.
fn has_traversal(s: &str) -> bool {
    s.starts_with('/')
        || s.contains('\0')
        || s.split('/').any(|c| c == "." || c == ".." || c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory StorageBackend for testing.
    struct MemStore {
        data: Mutex<HashMap<String, String>>,
    }

    impl MemStore {
        fn new() -> Self {
            Self {
                data: Mutex::new(HashMap::new()),
            }
        }
    }

    impl StorageBackend for MemStore {
        fn read(
            &self,
            secret_id: &str,
        ) -> anyhow::Result<Option<underwrap_protocol::message::SecretBytes>> {
            Ok(self
                .data
                .lock()
                .unwrap()
                .get(secret_id)
                .cloned()
                .map(Into::into))
        }

        fn write(&self, secret_id: &str, material: &[u8]) -> anyhow::Result<()> {
            self.data.lock().unwrap().insert(
                secret_id.to_string(),
                String::from_utf8(material.to_vec()).unwrap(),
            );
            Ok(())
        }

        fn list(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
            let data = self.data.lock().unwrap();
            let mut ids: Vec<String> = data
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect();
            ids.sort();
            Ok(ids)
        }

        fn delete(&self, secret_id: &str) -> anyhow::Result<bool> {
            Ok(self.data.lock().unwrap().remove(secret_id).is_some())
        }
    }

    const PREFIX: &str = "test-wrapper/libsecret/";
    const WRAPPER: &str = "test-wrapper";

    #[test]
    fn scoped_read_write() {
        let store = MemStore::new();

        // Write via channel handler.
        let resp = process_request(
            &SecretsdRequest::Write {
                secret_id: "default/item_1".to_string(),
                material: "secret_value".to_string().into(),
            },
            &store,
            PREFIX,
            WRAPPER,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Ok { .. }));

        // Verify it's stored with full prefix.
        assert_eq!(
            store.read("test-wrapper/libsecret/default/item_1").unwrap(),
            Some("secret_value".into())
        );

        // Read via channel handler.
        let resp = process_request(
            &SecretsdRequest::Read {
                secret_id: "default/item_1".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            AccessMode::ReadWrite,
        );
        match resp {
            SecretsdResponse::Ok { material, .. } => {
                assert_eq!(material.as_deref(), Some("secret_value"));
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn scoped_list_returns_relative_ids() {
        let store = MemStore::new();
        store
            .write("test-wrapper/libsecret/default/item_1", b"v1")
            .unwrap();
        store
            .write("test-wrapper/libsecret/default/item_2", b"v2")
            .unwrap();
        store
            .write("test-wrapper/libsecret/other/item_3", b"v3")
            .unwrap();
        // Different wrapper — should not appear.
        store
            .write("other-wrapper/libsecret/default/item_x", b"vx")
            .unwrap();

        // List all.
        let resp = process_request(
            &SecretsdRequest::List {
                prefix: "".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            AccessMode::ReadWrite,
        );
        match resp {
            SecretsdResponse::Ok { ids: Some(ids), .. } => {
                assert_eq!(ids.len(), 3);
                assert!(ids.contains(&"default/item_1".to_string()));
                assert!(ids.contains(&"default/item_2".to_string()));
                assert!(ids.contains(&"other/item_3".to_string()));
            }
            other => panic!("expected Ok with ids, got {other:?}"),
        }

        // List with sub-prefix.
        let resp = process_request(
            &SecretsdRequest::List {
                prefix: "default".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            AccessMode::ReadWrite,
        );
        match resp {
            SecretsdResponse::Ok { ids: Some(ids), .. } => {
                assert_eq!(ids.len(), 2);
            }
            other => panic!("expected Ok with ids, got {other:?}"),
        }
    }

    #[test]
    fn scoped_delete() {
        let store = MemStore::new();
        store
            .write("test-wrapper/libsecret/default/item_1", b"v1")
            .unwrap();

        let resp = process_request(
            &SecretsdRequest::Delete {
                secret_id: "default/item_1".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            AccessMode::ReadWrite,
        );
        match resp {
            SecretsdResponse::Ok {
                existed: Some(true),
                ..
            } => {}
            other => panic!("expected Ok with existed=true, got {other:?}"),
        }

        // Verify deleted.
        assert!(
            store
                .read("test-wrapper/libsecret/default/item_1")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn rejects_traversal_attempts() {
        let store = MemStore::new();

        // Write with .. should fail.
        let resp = process_request(
            &SecretsdRequest::Write {
                secret_id: "../other-wrapper/secret".to_string(),
                material: "hacked".to_string().into(),
            },
            &store,
            PREFIX,
            WRAPPER,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Error { .. }));

        // Read with absolute path should fail.
        let resp = process_request(
            &SecretsdRequest::Read {
                secret_id: "/etc/passwd".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Error { .. }));

        // Empty id should fail.
        let resp = process_request(
            &SecretsdRequest::Read {
                secret_id: "".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Error { .. }));

        // Double slash should fail.
        let resp = process_request(
            &SecretsdRequest::Read {
                secret_id: "default//item".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Error { .. }));

        // List with traversal should fail.
        let resp = process_request(
            &SecretsdRequest::List {
                prefix: "../../etc".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Error { .. }));
    }

    #[test]
    fn cannot_access_other_wrapper_secrets() {
        let store = MemStore::new();
        store
            .write("other-wrapper/libsecret/default/token", b"stolen")
            .unwrap();

        // Try to read another wrapper's secret — the prefix scoping prevents it.
        let resp = process_request(
            &SecretsdRequest::Read {
                secret_id: "../../other-wrapper/libsecret/default/token".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Error { .. }));
    }
}
