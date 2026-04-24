//! Directory credential channel handler — serves scoped StorageBackend operations
//! over an authenticated socketpair.
//!
//! The daemon creates one handler per directory credential binding during an
//! authenticated wrapper read. The handler scopes all operations to
//! `<credential-ref>/files/` and enforces the wrapper's access mode.

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::UnixStream;
use tracing::{debug, error, info, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{DirectoryErrorCode, DirectoryRequest, DirectoryResponse};
use underwrap_protocol::policy::PolicyDecision;
use underwrap_store::StorageBackend;

use crate::policy::{
    OP_STORE_WRITE, credential_resource, evaluate_after_approval_with, load_policy_document,
};

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
        "directory channel opened",
    );

    if let Err(e) =
        channel_loop(stream, store, &prefix, &wrapper_name, &credential_ref, access).await
    {
        debug!(credential = %credential_ref, err = %e, "directory channel closed");
    }

    info!(credential = %credential_ref, "directory channel handler exited");
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

        let response =
            process_request(&request, &*store, prefix, wrapper_name, credential_ref, access);
        write_frame_async(&mut stream, &response).await?;
    }
}

fn process_request(
    request: &DirectoryRequest,
    store: &dyn StorageBackend,
    prefix: &str,
    wrapper_name: &str,
    credential_ref: &str,
    access: AccessMode,
) -> DirectoryResponse {
    match request {
        DirectoryRequest::ListFiles => match store.list(prefix) {
            Ok(ids) => {
                let mut paths = Vec::new();
                let relative_prefix = format!("{prefix}/");
                for id in ids {
                    if let Some(path) = id.strip_prefix(&relative_prefix) {
                        paths.push(path.to_string());
                    }
                }
                paths.sort();
                debug!(credential = %credential_ref, count = paths.len(), "directory list");
                DirectoryResponse::ListFilesOk { paths }
            }
            Err(e) => {
                error!(credential = %credential_ref, err = %e, "directory list error");
                DirectoryResponse::Error {
                    code: DirectoryErrorCode::Internal,
                    message: format!("storage error: {e}"),
                }
            }
        },
        DirectoryRequest::ReadFile { path } => {
            let Some(full_id) = scoped_file_id(prefix, path) else {
                warn!(credential = %credential_ref, path = %path, "rejected invalid directory read");
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::InvalidRequest,
                    message: "invalid file path".to_string(),
                };
            };

            match store.read(&full_id) {
                Ok(content) => {
                    debug!(credential = %credential_ref, path = %path, "directory read");
                    DirectoryResponse::ReadFileOk { content }
                }
                Err(e) => {
                    error!(credential = %credential_ref, path = %path, err = %e, "directory read error");
                    DirectoryResponse::Error {
                        code: DirectoryErrorCode::Internal,
                        message: format!("storage error: {e}"),
                    }
                }
            }
        }
        DirectoryRequest::WriteFile { path, content } => {
            if access != AccessMode::ReadWrite {
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::Unauthorized,
                    message: "credential is read-only".to_string(),
                };
            }
            if let Err(message) =
                check_store_write_policy(store, wrapper_name, credential_ref, path)
            {
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::Unauthorized,
                    message,
                };
            }
            let Some(full_id) = scoped_file_id(prefix, path) else {
                warn!(credential = %credential_ref, path = %path, "rejected invalid directory write");
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::InvalidRequest,
                    message: "invalid file path".to_string(),
                };
            };

            match store.write(&full_id, content.as_ref()) {
                Ok(()) => {
                    debug!(credential = %credential_ref, path = %path, size = content.as_ref().len(), "directory write");
                    DirectoryResponse::WriteFileOk
                }
                Err(e) => {
                    error!(credential = %credential_ref, path = %path, err = %e, "directory write error");
                    DirectoryResponse::Error {
                        code: DirectoryErrorCode::Internal,
                        message: format!("storage error: {e}"),
                    }
                }
            }
        }
        DirectoryRequest::DeleteFile { path } => {
            if access != AccessMode::ReadWrite {
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::Unauthorized,
                    message: "credential is read-only".to_string(),
                };
            }
            if let Err(message) =
                check_store_write_policy(store, wrapper_name, credential_ref, path)
            {
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::Unauthorized,
                    message,
                };
            }
            let Some(full_id) = scoped_file_id(prefix, path) else {
                warn!(credential = %credential_ref, path = %path, "rejected invalid directory delete");
                return DirectoryResponse::Error {
                    code: DirectoryErrorCode::InvalidRequest,
                    message: "invalid file path".to_string(),
                };
            };

            match store.delete(&full_id) {
                Ok(existed) => {
                    debug!(credential = %credential_ref, path = %path, existed = existed, "directory delete");
                    DirectoryResponse::DeleteFileOk { existed }
                }
                Err(e) => {
                    error!(credential = %credential_ref, path = %path, err = %e, "directory delete error");
                    DirectoryResponse::Error {
                        code: DirectoryErrorCode::Internal,
                        message: format!("storage error: {e}"),
                    }
                }
            }
        }
    }
}

fn check_store_write_policy(
    store: &dyn StorageBackend,
    wrapper_name: &str,
    credential_ref: &str,
    path: &str,
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
            crate::audit::wrapper_write_denied(
                "directory",
                wrapper_name,
                credential_ref,
                path,
                &reason,
            );
            Err(reason)
        }
        PolicyDecision::NeedsApproval { .. } => {
            let reason = "per-call approval is not applicable to wrapper channels".to_string();
            crate::audit::wrapper_write_denied(
                "directory",
                wrapper_name,
                credential_ref,
                path,
                &reason,
            );
            Err(reason)
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

    const PREFIX: &str = "gws/state/files";
    const WRAPPER: &str = "gws-wrapper";
    const REF: &str = "gws/state";

    #[test]
    fn list_read_write_delete_roundtrip() {
        let store = MemStore::new();
        process_request(
            &DirectoryRequest::WriteFile {
                path: "config.json".to_string(),
                content: b"{}".as_slice().into(),
            },
            &store,
            PREFIX,
            WRAPPER,
            REF,
            AccessMode::ReadWrite,
        );
        process_request(
            &DirectoryRequest::WriteFile {
                path: "sub/token.bin".to_string(),
                content: b"abc".as_slice().into(),
            },
            &store,
            PREFIX,
            WRAPPER,
            REF,
            AccessMode::ReadWrite,
        );

        let resp = process_request(
            &DirectoryRequest::ListFiles,
            &store,
            PREFIX,
            WRAPPER,
            REF,
            AccessMode::ReadWrite,
        );
        match resp {
            DirectoryResponse::ListFilesOk { paths } => {
                assert_eq!(paths, vec!["config.json", "sub/token.bin"]);
            }
            other => panic!("expected ListFilesOk, got {other:?}"),
        }

        let resp = process_request(
            &DirectoryRequest::ReadFile {
                path: "sub/token.bin".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            REF,
            AccessMode::ReadWrite,
        );
        match resp {
            DirectoryResponse::ReadFileOk { content } => {
                assert_eq!(content, Some(b"abc".as_slice().into()));
            }
            other => panic!("expected ReadFileOk, got {other:?}"),
        }

        let resp = process_request(
            &DirectoryRequest::DeleteFile {
                path: "config.json".to_string(),
            },
            &store,
            PREFIX,
            WRAPPER,
            REF,
            AccessMode::ReadWrite,
        );
        assert!(matches!(
            resp,
            DirectoryResponse::DeleteFileOk { existed: true }
        ));
    }

    #[test]
    fn rejects_invalid_paths() {
        let store = MemStore::new();
        for path in ["", "/etc/passwd", "../x", "a//b", "a/./b"] {
            let resp = process_request(
                &DirectoryRequest::ReadFile {
                    path: path.to_string(),
                },
                &store,
                PREFIX,
                WRAPPER,
                REF,
                AccessMode::ReadWrite,
            );
            assert!(matches!(
                resp,
                DirectoryResponse::Error {
                    code: DirectoryErrorCode::InvalidRequest,
                    ..
                }
            ));
        }
    }

    #[test]
    fn read_only_rejects_mutation() {
        let store = MemStore::new();
        let resp = process_request(
            &DirectoryRequest::WriteFile {
                path: "config.json".to_string(),
                content: b"{}".as_slice().into(),
            },
            &store,
            PREFIX,
            WRAPPER,
            REF,
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
