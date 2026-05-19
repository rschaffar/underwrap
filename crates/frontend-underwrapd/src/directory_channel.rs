//! Directory resource channel handler — serves scoped operations over an
//! authenticated socketpair.
//!
//! Initial reads are served from the one-shot bundle snapshot fetched during
//! wrapper authorization. Mutations buffer in the in-memory snapshot and are
//! persisted as a single `resources/<id>` blob write when the channel closes,
//! provided backend policy granted write access for the files slot.

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
use underwrap_store::{StorageBackend, blob_from_resource_entries, save_resource_blob};

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
    let dirty = Arc::new(Mutex::new(false));
    if let Err(e) = channel_loop(
        stream,
        store.clone(),
        entries.clone(),
        dirty.clone(),
        &prefix,
        &wrapper_name,
        &resource_id,
        access,
    )
    .await
    {
        debug!(resource = %resource_id, err = %e, "directory channel closed");
    }

    let should_flush = *dirty.lock().unwrap();
    if should_flush {
        let snapshot = entries.lock().unwrap().clone();
        match blob_from_resource_entries(&resource_id, &snapshot) {
            Ok(blob) => match save_resource_blob(&*store, &resource_id, &blob) {
                Ok(()) => debug!(
                    wrapper = %wrapper_name,
                    resource = %resource_id,
                    "flushed directory changes to resource blob",
                ),
                Err(e) => error!(
                    wrapper = %wrapper_name,
                    resource = %resource_id,
                    err = %e,
                    "saving resource blob after directory channel close failed",
                ),
            },
            Err(e) => error!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                err = %e,
                "rebuilding resource blob after directory channel close failed",
            ),
        }
    }

    info!(resource = %resource_id, "directory channel handler exited");
}

#[allow(clippy::too_many_arguments)]
async fn channel_loop(
    mut stream: UnixStream,
    store: Arc<dyn StorageBackend>,
    entries: Arc<Mutex<HashMap<String, SecretBytes>>>,
    dirty: Arc<Mutex<bool>>,
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
            &dirty,
            prefix,
            wrapper_name,
            resource_id,
            access,
        );
        write_frame_async(&mut stream, &response).await?;
    }
}

#[allow(clippy::too_many_arguments)]
fn process_request(
    request: &DirectoryRequest,
    _store: &dyn StorageBackend,
    entries: &Mutex<HashMap<String, SecretBytes>>,
    dirty: &Mutex<bool>,
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

            entries
                .lock()
                .unwrap()
                .insert(full_id.clone(), content.clone());
            *dirty.lock().unwrap() = true;
            debug!(resource = %resource_id, path = %path, size = content.as_ref().len(), "directory write");
            DirectoryResponse::WriteFileOk
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

            let existed = entries.lock().unwrap().remove(&full_id).is_some();
            if existed {
                *dirty.lock().unwrap() = true;
            }
            debug!(resource = %resource_id, path = %path, existed = existed, "directory delete");
            DirectoryResponse::DeleteFileOk { existed }
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

    fn dirty() -> Mutex<bool> {
        Mutex::new(false)
    }

    #[test]
    fn list_and_read_use_snapshot() {
        let store = MemStore::with_entries(&[]);
        let entries = entries(&[("gws/state/files/token", b"abc")]);
        let dirty = dirty();
        let resp = process_request(
            &DirectoryRequest::ListFiles,
            &store,
            &entries,
            &dirty,
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
            &dirty,
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
        assert!(!*dirty.lock().unwrap());
    }

    #[test]
    fn write_requires_backend_policy_derived_access() {
        let store = MemStore::with_entries(&[]);
        let entries = entries(&[]);
        let dirty = dirty();
        let resp = process_request(
            &DirectoryRequest::WriteFile {
                path: "token".to_string(),
                content: SecretBytes::from("abc"),
            },
            &store,
            &entries,
            &dirty,
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
