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
use underwrap_protocol::policy::PolicyDecision;
use underwrap_store::StorageBackend;

use crate::policy::{
    OP_STORE_WRITE, credential_resource, evaluate_after_approval_with, load_policy_document,
};

/// Run the secretsd channel handler loop on the daemon's end of the socketpair.
///
/// All operations are scoped to `{prefix}` which is set to
/// `<credential-ref>` at creation time. The secretsd process sends relative
/// IDs; this handler prepends the prefix before calling the StorageBackend.
///
/// Returns when the channel is closed (secretsd exited / namespace died).
pub async fn run_channel(
    stream: UnixStream,
    store: Arc<dyn StorageBackend>,
    prefix: String,
    wrapper_name: String,
    credential_ref: String,
    access: AccessMode,
) {
    info!(
        wrapper = %wrapper_name,
        credential = %credential_ref,
        prefix = %prefix,
        "secretsd channel opened",
    );

    if let Err(e) =
        channel_loop(stream, store, &prefix, &wrapper_name, &credential_ref, access).await
    {
        debug!(wrapper = %wrapper_name, err = %e, "secretsd channel closed");
    }

    info!(wrapper = %wrapper_name, "secretsd channel handler exited");
}

async fn channel_loop(
    mut stream: UnixStream,
    store: Arc<dyn StorageBackend>,
    prefix: &str,
    wrapper_name: &str,
    credential_ref: &str,
    access: AccessMode,
) -> Result<()> {
    loop {
        let request: SecretsdRequest = match read_frame_async(&mut stream).await {
            Ok(req) => req,
            Err(e) => {
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

        let response =
            process_request(&request, &*store, prefix, wrapper_name, credential_ref, access);
        write_frame_async(&mut stream, &response).await?;
    }
}

fn process_request(
    request: &SecretsdRequest,
    store: &dyn StorageBackend,
    prefix: &str,
    wrapper_name: &str,
    credential_ref: &str,
    access: AccessMode,
) -> SecretsdResponse {
    match request {
        SecretsdRequest::Read { secret_id } => {
            let Some(full_id) = scoped_id(prefix, secret_id) else {
                warn!(wrapper = %wrapper_name, id = %secret_id, "rejected invalid secretsd read");
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::InvalidRequest,
                    message: "invalid secret id".to_string(),
                };
            };

            match store.read(&full_id) {
                Ok(material) => {
                    debug!(wrapper = %wrapper_name, id = %full_id, "secretsd read");
                    crate::audit::secretsd_read(wrapper_name, secret_id);
                    SecretsdResponse::ReadOk { material }
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
            if let Err(message) =
                check_store_write_policy(store, wrapper_name, credential_ref, secret_id, "libsecret")
            {
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::Unauthorized,
                    message,
                };
            }
            let Some(full_id) = scoped_id(prefix, secret_id) else {
                warn!(wrapper = %wrapper_name, id = %secret_id, "rejected invalid secretsd write");
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::InvalidRequest,
                    message: "invalid secret id".to_string(),
                };
            };

            match store.write(&full_id, material.as_ref()) {
                Ok(()) => {
                    debug!(wrapper = %wrapper_name, id = %full_id, "secretsd write");
                    crate::audit::secretsd_write(wrapper_name, secret_id);
                    SecretsdResponse::WriteOk
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
                if has_traversal(rel_prefix) {
                    warn!(wrapper = %wrapper_name, prefix = %rel_prefix, "rejected invalid secretsd list prefix");
                    return SecretsdResponse::Error {
                        code: SecretsdErrorCode::InvalidRequest,
                        message: "invalid prefix".to_string(),
                    };
                }
                format!("{prefix}/{rel_prefix}")
            };

            match store.list(&full_prefix) {
                Ok(ids) => {
                    let relative_prefix = format!("{prefix}/");
                    let relative_ids: Vec<String> = ids
                        .into_iter()
                        .filter_map(|id| id.strip_prefix(&relative_prefix).map(|s| s.to_string()))
                        .collect();
                    debug!(wrapper = %wrapper_name, prefix = %full_prefix, count = relative_ids.len(), "secretsd list");
                    SecretsdResponse::ListOk { ids: relative_ids }
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
            if let Err(message) =
                check_store_write_policy(store, wrapper_name, credential_ref, secret_id, "libsecret")
            {
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::Unauthorized,
                    message,
                };
            }
            let Some(full_id) = scoped_id(prefix, secret_id) else {
                warn!(wrapper = %wrapper_name, id = %secret_id, "rejected invalid secretsd delete");
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::InvalidRequest,
                    message: "invalid secret id".to_string(),
                };
            };

            match store.delete(&full_id) {
                Ok(existed) => {
                    debug!(wrapper = %wrapper_name, id = %full_id, existed = existed, "secretsd delete");
                    crate::audit::secretsd_delete(wrapper_name, secret_id);
                    SecretsdResponse::DeleteOk { existed }
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

/// Evaluate the `store.write` policy gate for a wrapper-initiated mutation
/// against the credential the channel was opened for. Returns Ok if the
/// write is permitted, otherwise an Err carrying the message to surface to
/// the secretsd peer (the same string is also logged via audit).
///
/// The channel was authenticated when the wrapper request opened it, so the
/// mode gate is bypassed (`evaluate_after_approval_with`). Only the rule and
/// grant layer applies.
fn check_store_write_policy(
    store: &dyn StorageBackend,
    wrapper_name: &str,
    credential_ref: &str,
    item: &str,
    kind: &str,
) -> std::result::Result<(), String> {
    let document = match load_policy_document(store) {
        Ok(doc) => doc,
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "failed to load policy document for store.write gate");
            return Err("internal policy error".to_string());
        }
    };
    let resource = credential_resource(credential_ref);
    match evaluate_after_approval_with(&document, wrapper_name, &resource, OP_STORE_WRITE) {
        PolicyDecision::Allow => Ok(()),
        PolicyDecision::Deny { reason } => {
            crate::audit::wrapper_write_denied(kind, wrapper_name, credential_ref, item, &reason);
            Err(reason)
        }
        PolicyDecision::NeedsApproval { .. } => {
            // Channel was authenticated at creation; per-call approval cannot
            // be solved on a long-lived helper channel. Treat as deny.
            let reason = "per-call approval is not applicable to wrapper channels".to_string();
            crate::audit::wrapper_write_denied(kind, wrapper_name, credential_ref, item, &reason);
            Err(reason)
        }
    }
}

/// Construct a scoped secret ID by prepending the prefix.
/// Returns None if the relative ID contains traversal components.
fn scoped_id(prefix: &str, relative_id: &str) -> Option<String> {
    if relative_id.is_empty() || has_traversal(relative_id) {
        return None;
    }
    Some(format!("{prefix}/{relative_id}"))
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
    use crate::test_support::MemStore;

    const PREFIX: &str = "test-wrapper/libsecret";
    const WRAPPER: &str = "test-wrapper";
    const CREDENTIAL: &str = "test-wrapper/libsecret";

    #[test]
    fn scoped_read_write() {
        let store = MemStore::new();

        let resp = process_request(
            &SecretsdRequest::Write {
                secret_id: "default/item_1".to_string(),
                material: "secret_value".into(),
            },
            &store,
            PREFIX,
            WRAPPER,
            CREDENTIAL,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::WriteOk));

        assert_eq!(
            store.read("test-wrapper/libsecret/default/item_1").unwrap(),
            Some("secret_value".into())
        );

        let resp = process_request(
            &SecretsdRequest::Read {
                secret_id: "default/item_1".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            CREDENTIAL,
            AccessMode::ReadWrite,
        );
        match resp {
            SecretsdResponse::ReadOk { material } => {
                assert_eq!(material, Some("secret_value".into()));
            }
            other => panic!("expected ReadOk, got {other:?}"),
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
        store
            .write("other-wrapper/libsecret/default/item_x", b"vx")
            .unwrap();

        let resp = process_request(
            &SecretsdRequest::List {
                prefix: "".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            CREDENTIAL,
            AccessMode::ReadWrite,
        );
        match resp {
            SecretsdResponse::ListOk { ids } => {
                assert_eq!(ids.len(), 3);
                assert!(ids.contains(&"default/item_1".to_string()));
                assert!(ids.contains(&"default/item_2".to_string()));
                assert!(ids.contains(&"other/item_3".to_string()));
            }
            other => panic!("expected ListOk, got {other:?}"),
        }

        let resp = process_request(
            &SecretsdRequest::List {
                prefix: "default".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            CREDENTIAL,
            AccessMode::ReadWrite,
        );
        match resp {
            SecretsdResponse::ListOk { ids } => {
                assert_eq!(ids.len(), 2);
            }
            other => panic!("expected ListOk, got {other:?}"),
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
            CREDENTIAL,
            AccessMode::ReadWrite,
        );
        match resp {
            SecretsdResponse::DeleteOk { existed } => assert!(existed),
            other => panic!("expected DeleteOk, got {other:?}"),
        }

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

        let resp = process_request(
            &SecretsdRequest::Write {
                secret_id: "../other-wrapper/secret".to_string(),
                material: "hacked".into(),
            },
            &store,
            PREFIX,
            WRAPPER,
            CREDENTIAL,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Error { .. }));

        let resp = process_request(
            &SecretsdRequest::Read {
                secret_id: "/etc/passwd".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            CREDENTIAL,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Error { .. }));

        let resp = process_request(
            &SecretsdRequest::Read {
                secret_id: "".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            CREDENTIAL,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Error { .. }));

        let resp = process_request(
            &SecretsdRequest::Read {
                secret_id: "default//item".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            CREDENTIAL,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Error { .. }));

        let resp = process_request(
            &SecretsdRequest::List {
                prefix: "../../etc".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            CREDENTIAL,
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

        let resp = process_request(
            &SecretsdRequest::Read {
                secret_id: "../../other-wrapper/libsecret/default/token".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            CREDENTIAL,
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, SecretsdResponse::Error { .. }));
    }
}
