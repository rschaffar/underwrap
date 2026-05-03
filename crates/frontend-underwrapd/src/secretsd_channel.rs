//! Secretsd channel handler — serves scoped StorageBackend operations over an
//! authenticated socketpair.
//!
//! Initial reads are served from the one-shot bundle snapshot fetched during
//! wrapper authorization. Mutations are written back only when backend policy
//! granted write access for the libsecret slot.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::net::UnixStream;
use tracing::{debug, error, info, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{SecretBytes, SecretsdErrorCode, SecretsdRequest, SecretsdResponse};
use underwrap_protocol::resource::relative_to_prefix;
use underwrap_store::StorageBackend;

/// Run the secretsd channel handler loop on the daemon's end of the socketpair.
pub async fn run_channel(
    stream: UnixStream,
    store: Arc<dyn StorageBackend>,
    prefix: String,
    entries: HashMap<String, SecretBytes>,
    wrapper_name: String,
    resource_id: String,
    access: AccessMode,
) {
    info!(
        wrapper = %wrapper_name,
        resource = %resource_id,
        prefix = %prefix,
        ?access,
        "secretsd channel opened",
    );

    let entries = Arc::new(Mutex::new(entries));
    if let Err(e) = channel_loop(
        stream,
        store,
        entries,
        &prefix,
        &wrapper_name,
        &resource_id,
        access,
    )
    .await
    {
        debug!(wrapper = %wrapper_name, err = %e, "secretsd channel closed");
    }

    info!(wrapper = %wrapper_name, "secretsd channel handler exited");
}

async fn channel_loop(
    mut stream: UnixStream,
    store: Arc<dyn StorageBackend>,
    entries: Arc<Mutex<HashMap<String, SecretBytes>>>,
    prefix: &str,
    wrapper_name: &str,
    resource_id: &str,
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

        let response = process_request(
            &request,
            &*store,
            &entries,
            prefix,
            wrapper_name,
            resource_id,
            access,
        );
        write_frame_async(&mut stream, &response).await?;
    }
}

fn process_request(
    request: &SecretsdRequest,
    store: &dyn StorageBackend,
    entries: &Mutex<HashMap<String, SecretBytes>>,
    prefix: &str,
    wrapper_name: &str,
    resource_id: &str,
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

            let material = entries.lock().unwrap().get(&full_id).cloned();
            debug!(wrapper = %wrapper_name, id = %full_id, "secretsd read");
            crate::audit::secretsd_read(wrapper_name, secret_id);
            SecretsdResponse::ReadOk { material }
        }

        SecretsdRequest::Write { secret_id, material } => {
            if access != AccessMode::ReadWrite {
                crate::audit::wrapper_write_denied(
                    "libsecret",
                    wrapper_name,
                    resource_id,
                    secret_id,
                    "libsecret slot is not write-authorized by backend policy",
                );
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::Unauthorized,
                    message: "libsecret slot is read-only by backend policy".to_string(),
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
                    entries.lock().unwrap().insert(full_id.clone(), material.clone());
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

            let mut relative_ids: Vec<String> = entries
                .lock()
                .unwrap()
                .keys()
                .filter_map(|id| relative_to_prefix(id, prefix).map(ToString::to_string))
                .filter(|id| {
                    rel_prefix.is_empty()
                        || id == rel_prefix
                        || id.starts_with(&format!("{rel_prefix}/"))
                })
                .collect();
            relative_ids.sort();
            debug!(wrapper = %wrapper_name, prefix = %full_prefix, count = relative_ids.len(), "secretsd list");
            SecretsdResponse::ListOk { ids: relative_ids }
        }

        SecretsdRequest::Delete { secret_id } => {
            if access != AccessMode::ReadWrite {
                crate::audit::wrapper_write_denied(
                    "libsecret",
                    wrapper_name,
                    resource_id,
                    secret_id,
                    "libsecret slot is not write-authorized by backend policy",
                );
                return SecretsdResponse::Error {
                    code: SecretsdErrorCode::Unauthorized,
                    message: "libsecret slot is read-only by backend policy".to_string(),
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
                    entries.lock().unwrap().remove(&full_id);
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

fn scoped_id(prefix: &str, secret_id: &str) -> Option<String> {
    if has_traversal(secret_id) {
        None
    } else {
        Some(format!("{prefix}/{secret_id}"))
    }
}

fn has_traversal(path: &str) -> bool {
    path.is_empty()
        || path.starts_with('/')
        || path.contains('\0')
        || path
            .split('/')
            .any(|c| c == "." || c == ".." || c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MemStore;

    fn entries(items: &[(&str, &[u8])]) -> Mutex<HashMap<String, SecretBytes>> {
        Mutex::new(
            items
                .iter()
                .map(|(k, v)| ((*k).to_string(), SecretBytes::from(*v)))
                .collect(),
        )
    }

    #[test]
    fn read_uses_snapshot() {
        let store = MemStore::with_entries(&[]);
        let entries = entries(&[("gh/libsecret/default/item.secret", b"s")]);
        let resp = process_request(
            &SecretsdRequest::Read {
                secret_id: "default/item.secret".to_string(),
            },
            &store,
            &entries,
            "gh/libsecret",
            "wrapper",
            "gh",
            AccessMode::ReadOnly,
        );
        assert_eq!(
            resp,
            SecretsdResponse::ReadOk {
                material: Some(SecretBytes::from("s"))
            }
        );
    }

    #[test]
    fn write_requires_backend_policy_derived_access() {
        let store = MemStore::with_entries(&[]);
        let entries = entries(&[]);
        let resp = process_request(
            &SecretsdRequest::Write {
                secret_id: "default/item.secret".to_string(),
                material: SecretBytes::from("s"),
            },
            &store,
            &entries,
            "gh/libsecret",
            "wrapper",
            "gh",
            AccessMode::ReadOnly,
        );
        assert!(matches!(
            resp,
            SecretsdResponse::Error {
                code: SecretsdErrorCode::Unauthorized,
                ..
            }
        ));
    }
}
