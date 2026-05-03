//! Directory resource channel handler — serves scoped operations over an
//! authenticated socketpair.
//!
//! Initial reads are served from the one-shot bundle snapshot fetched during
//! wrapper authorization. Mutations are written back to the backend store only
//! when the backend policy-derived access mode is read-write.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tokio::net::UnixStream;
use tracing::{debug, error, info, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{
    DirectoryErrorCode, DirectoryRequest, DirectoryResponse, SecretBytes,
};
use underwrap_protocol::resource::relative_to_prefix;
use underwrap_store::StorageBackend;

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
        "directory channel opened",
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
        debug!(resource = %resource_id, err = %e, "directory channel closed");
    }

    info!(resource = %resource_id, "directory channel handler exited");
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
        let request: DirectoryRequest = match read_frame_async(&mut stream).await {
            Ok(req) => req,
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("unexpected eof")
                    || msg.contains("connection reset")
                    || msg.contains("broken pipe")
                {
                    return Ok(());
                }
                return Err(e).context("reading directory request");
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
    request: &DirectoryRequest,
    store: &dyn StorageBackend,
    entries: &Mutex<HashMap<String, SecretBytes>>,
    prefix: &str,
    wrapper_name: &str,
    resource_id: &str,
    access: AccessMode,
) -> DirectoryResponse {
    match request {
        DirectoryRequest::ListFiles => {
            let mut paths: Vec<String> = entries
                .lock()
                .unwrap()
                .keys()
                .filter_map(|id| relative_to_prefix(id, prefix).map(ToString::to_string))
                .collect();
            paths.sort();
            debug!(resource = %resource_id, count = paths.len(), "directory list");
            DirectoryResponse::ListFilesOk { paths }
        }
        DirectoryRequest::ReadFile { path } => {
            let Some(full_id) = scoped_file_id(prefix, path) else {
                warn!(resource = %resource_id, path = %path, "rejected invalid directory read");
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::InvalidRequest,
                    message: "invalid file path".to_string(),
                };
            };

            let material = entries.lock().unwrap().get(&full_id).cloned();
            debug!(resource = %resource_id, path = %path, "directory read");
            DirectoryResponse::ReadFileOk { content: material }
        }
        DirectoryRequest::WriteFile { path, content } => {
            if access != AccessMode::ReadWrite {
                crate::audit::wrapper_write_denied(
                    "files",
                    wrapper_name,
                    resource_id,
                    path,
                    "files slot is not write-authorized by backend policy",
                );
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::Unauthorized,
                    message: "files slot is read-only by backend policy".to_string(),
                };
            }
            let Some(full_id) = scoped_file_id(prefix, path) else {
                warn!(resource = %resource_id, path = %path, "rejected invalid directory write");
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::InvalidRequest,
                    message: "invalid file path".to_string(),
                };
            };

            match store.write(&full_id, content.as_ref()) {
                Ok(()) => {
                    entries
                        .lock()
                        .unwrap()
                        .insert(full_id.clone(), content.clone());
                    debug!(resource = %resource_id, path = %path, size = content.as_ref().len(), "directory write");
                    DirectoryResponse::WriteFileOk
                }
                Err(e) => {
                    error!(resource = %resource_id, path = %path, err = %e, "directory write error");
                    DirectoryResponse::Error {
                        code: DirectoryErrorCode::Internal,
                        message: format!("storage error: {e}"),
                    }
                }
            }
        }
        DirectoryRequest::DeleteFile { path } => {
            if access != AccessMode::ReadWrite {
                crate::audit::wrapper_write_denied(
                    "files",
                    wrapper_name,
                    resource_id,
                    path,
                    "files slot is not write-authorized by backend policy",
                );
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::Unauthorized,
                    message: "files slot is read-only by backend policy".to_string(),
                };
            }
            let Some(full_id) = scoped_file_id(prefix, path) else {
                warn!(resource = %resource_id, path = %path, "rejected invalid directory delete");
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::InvalidRequest,
                    message: "invalid file path".to_string(),
                };
            };

            match store.delete(&full_id) {
                Ok(existed) => {
                    entries.lock().unwrap().remove(&full_id);
                    debug!(resource = %resource_id, path = %path, existed = existed, "directory delete");
                    DirectoryResponse::DeleteFileOk { existed }
                }
                Err(e) => {
                    error!(resource = %resource_id, path = %path, err = %e, "directory delete error");
                    DirectoryResponse::Error {
                        code: DirectoryErrorCode::Internal,
                        message: format!("storage error: {e}"),
                    }
                }
            }
        }
    }
}

fn scoped_file_id(prefix: &str, path: &str) -> Option<String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\0')
        || path
            .split('/')
            .any(|c| c == "." || c == ".." || c.is_empty())
    {
        return None;
    }
    Some(format!("{prefix}/{path}"))
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
    fn list_and_read_use_snapshot() {
        let store = MemStore::with_entries(&[]);
        let entries = entries(&[("gws/state/files/token", b"abc")]);
        let resp = process_request(
            &DirectoryRequest::ListFiles,
            &store,
            &entries,
            "gws/state/files",
            "wrapper",
            "gws/state",
            AccessMode::ReadOnly,
        );
        assert_eq!(
            resp,
            DirectoryResponse::ListFilesOk {
                paths: vec!["token".to_string()]
            }
        );
        let resp = process_request(
            &DirectoryRequest::ReadFile {
                path: "token".to_string(),
            },
            &store,
            &entries,
            "gws/state/files",
            "wrapper",
            "gws/state",
            AccessMode::ReadOnly,
        );
        assert_eq!(
            resp,
            DirectoryResponse::ReadFileOk {
                content: Some(SecretBytes::from("abc"))
            }
        );
    }

    #[test]
    fn write_requires_backend_policy_derived_access() {
        let store = MemStore::with_entries(&[]);
        let entries = entries(&[]);
        let resp = process_request(
            &DirectoryRequest::WriteFile {
                path: "token".to_string(),
                content: SecretBytes::from("abc"),
            },
            &store,
            &entries,
            "gws/state/files",
            "wrapper",
            "gws/state",
            AccessMode::ReadOnly,
        );
        assert!(matches!(
            resp,
            DirectoryResponse::Error {
                code: DirectoryErrorCode::Unauthorized,
                ..
            }
        ));
    }
}
