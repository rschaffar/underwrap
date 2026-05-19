//! Wrapper-resource channel handler — serves the bulk-blob IPC verbs
//! (`Checkout`, `Checkin`, `UsageSignal`) for one resource over an
//! authenticated socketpair.
//!
//! The channel is scoped to one `(wrapper, resource)` pair and one
//! grant-derived access mode. Authorization happened at channel setup;
//! the handler simply enforces the access mode on `Checkin` and serves
//! reads / usage signals unconditionally.
//!
//! Persistence is one `resources/<id>` blob write per accepted
//! `Checkin`. The blob bytes are validated as JCS-canonical before any
//! write hits the backend — the same canonicality contract enforced by
//! the admin PUT endpoint.

use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::net::UnixStream;
use tracing::{debug, error, info, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{
    ResourceErrorCode, WrapperResourceRequest, WrapperResourceResponse,
};
use underwrap_protocol::resource::resource_blob_key;
use underwrap_protocol::resource_blob::ResourceBlob;
use underwrap_store::{StorageBackend, load_resource_blob, save_resource_blob};

/// Run the wrapper-resource channel handler loop on the daemon end of
/// the socketpair. Returns when the wrapper end closes or a fatal
/// protocol error occurs.
pub async fn run_channel(
    stream: UnixStream,
    store: Arc<dyn StorageBackend>,
    wrapper_name: String,
    resource_id: String,
    access: AccessMode,
) {
    info!(
        wrapper = %wrapper_name,
        resource = %resource_id,
        ?access,
        "wrapper-resource channel opened",
    );
    if let Err(e) = channel_loop(stream, &*store, &wrapper_name, &resource_id, access).await {
        debug!(
            wrapper = %wrapper_name,
            resource = %resource_id,
            err = %e,
            "wrapper-resource channel closed",
        );
    }
    info!(
        wrapper = %wrapper_name,
        resource = %resource_id,
        "wrapper-resource channel handler exited",
    );
}

async fn channel_loop(
    mut stream: UnixStream,
    store: &dyn StorageBackend,
    wrapper_name: &str,
    resource_id: &str,
    access: AccessMode,
) -> Result<()> {
    loop {
        let req: WrapperResourceRequest = match read_frame_async(&mut stream).await {
            Ok(req) => req,
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("unexpected eof")
                    || msg.contains("connection reset")
                    || msg.contains("broken pipe")
                {
                    return Ok(());
                }
                return Err(e).context("reading wrapper-resource request");
            }
        };
        let resp = handle_request(req, store, wrapper_name, resource_id, access);
        write_frame_async(&mut stream, &resp).await?;
    }
}

fn handle_request(
    request: WrapperResourceRequest,
    store: &dyn StorageBackend,
    wrapper_name: &str,
    resource_id: &str,
    access: AccessMode,
) -> WrapperResourceResponse {
    match request {
        WrapperResourceRequest::Checkout => {
            handle_checkout(store, wrapper_name, resource_id, access)
        }
        WrapperResourceRequest::Checkin { blob } => {
            handle_checkin(blob, store, wrapper_name, resource_id, access)
        }
        WrapperResourceRequest::UsageSignal => {
            handle_usage_signal(store, wrapper_name, resource_id)
        }
    }
}

fn handle_checkout(
    store: &dyn StorageBackend,
    wrapper_name: &str,
    resource_id: &str,
    access: AccessMode,
) -> WrapperResourceResponse {
    let blob = match load_resource_blob(store, resource_id) {
        Ok(b) => b,
        Err(e) => {
            error!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                err = %e,
                "loading blob on checkout",
            );
            return WrapperResourceResponse::Error {
                code: ResourceErrorCode::Internal,
                message: "loading resource blob failed".to_string(),
            };
        }
    };
    let bytes = match blob.to_canonical_bytes() {
        Ok(b) => b,
        Err(e) => {
            error!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                err = %e,
                "canonicalizing blob on checkout",
            );
            return WrapperResourceResponse::Error {
                code: ResourceErrorCode::Internal,
                message: "canonicalizing resource blob failed".to_string(),
            };
        }
    };
    debug!(
        wrapper = %wrapper_name,
        resource = %resource_id,
        bytes = bytes.len(),
        "wrapper-resource checkout",
    );
    crate::audit::resource_checkout(wrapper_name, resource_id);
    WrapperResourceResponse::CheckoutOk {
        blob: bytes,
        writable: access == AccessMode::ReadWrite,
    }
}

fn handle_checkin(
    blob: Vec<u8>,
    store: &dyn StorageBackend,
    wrapper_name: &str,
    resource_id: &str,
    access: AccessMode,
) -> WrapperResourceResponse {
    if access != AccessMode::ReadWrite {
        crate::audit::wrapper_write_denied(
            "resource",
            wrapper_name,
            resource_id,
            "<full-blob>",
            "resource is not write-authorized by backend policy",
        );
        return WrapperResourceResponse::Error {
            code: ResourceErrorCode::Unauthorized,
            message: "resource is read-only by backend policy".to_string(),
        };
    }
    let decoded = match ResourceBlob::from_canonical_bytes(&blob) {
        Ok(b) => b,
        Err(e) => {
            warn!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                err = %e,
                "rejected non-decodable blob on checkin",
            );
            return WrapperResourceResponse::Error {
                code: ResourceErrorCode::InvalidRequest,
                message: "invalid resource blob".to_string(),
            };
        }
    };
    match decoded.to_canonical_bytes() {
        Ok(canonical) if canonical == blob => {}
        Ok(_) => {
            warn!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                "rejected non-canonical blob on checkin",
            );
            return WrapperResourceResponse::Error {
                code: ResourceErrorCode::InvalidRequest,
                message: "noncanonical_blob".to_string(),
            };
        }
        Err(e) => {
            error!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                err = %e,
                "canonicalizing blob on checkin",
            );
            return WrapperResourceResponse::Error {
                code: ResourceErrorCode::Internal,
                message: "canonicalizing blob failed".to_string(),
            };
        }
    }
    match save_resource_blob(store, resource_id, &decoded) {
        Ok(()) => {
            debug!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                "wrapper-resource checkin",
            );
            crate::audit::resource_checkin(wrapper_name, resource_id);
            WrapperResourceResponse::CheckinOk
        }
        Err(e) => {
            error!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                err = %e,
                "saving blob on checkin",
            );
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Internal,
                message: "saving resource blob failed".to_string(),
            }
        }
    }
}

fn handle_usage_signal(
    store: &dyn StorageBackend,
    wrapper_name: &str,
    resource_id: &str,
) -> WrapperResourceResponse {
    // A no-op read of the resource blob key. The backend bumps
    // `last_used_at` on the matching `(wrapper, resource)` pair as a
    // side effect of runtime read gating, so this read is enough to
    // record the usage without sending any blob bytes.
    let key = resource_blob_key(resource_id);
    match store.read(&key) {
        Ok(_) => {
            debug!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                "wrapper-resource usage signal",
            );
            crate::audit::resource_usage_signal(wrapper_name, resource_id);
            WrapperResourceResponse::UsageSignalOk
        }
        Err(e) => {
            error!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                err = %e,
                "usage signal storage read",
            );
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Internal,
                message: "usage signal failed".to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::MemStore;
    use std::collections::BTreeMap;
    use underwrap_protocol::message::SecretBytes;

    fn populated_blob() -> ResourceBlob {
        let mut blob = ResourceBlob::empty();
        let mut env = BTreeMap::new();
        env.insert("GH_TOKEN".into(), SecretBytes::from("ghp_old"));
        blob.env = env;
        blob
    }

    #[test]
    fn checkout_returns_canonical_bytes_and_writability() {
        let store = MemStore::with_entries(&[("github/token/env/GH_TOKEN", b"ghp_old")]);
        let resp = handle_checkout(&store, "wrapper", "github/token", AccessMode::ReadWrite);
        match resp {
            WrapperResourceResponse::CheckoutOk { blob, writable } => {
                assert!(writable);
                let decoded = ResourceBlob::from_canonical_bytes(&blob).unwrap();
                assert_eq!(decoded, populated_blob());
                // Bytes are byte-equal to a fresh canonicalization.
                assert_eq!(decoded.to_canonical_bytes().unwrap(), blob);
            }
            other => panic!("expected CheckoutOk, got {other:?}"),
        }
    }

    #[test]
    fn checkout_reports_readonly_when_grant_is_readonly() {
        let store = MemStore::with_entries(&[]);
        let resp = handle_checkout(&store, "wrapper", "github/token", AccessMode::ReadOnly);
        match resp {
            WrapperResourceResponse::CheckoutOk { writable, .. } => assert!(!writable),
            other => panic!("expected CheckoutOk, got {other:?}"),
        }
    }

    #[test]
    fn checkout_returns_empty_blob_for_unknown_resource() {
        let store = MemStore::with_entries(&[]);
        let resp = handle_checkout(&store, "wrapper", "missing", AccessMode::ReadOnly);
        match resp {
            WrapperResourceResponse::CheckoutOk { blob, .. } => {
                let decoded = ResourceBlob::from_canonical_bytes(&blob).unwrap();
                assert_eq!(decoded, ResourceBlob::empty());
            }
            other => panic!("expected CheckoutOk, got {other:?}"),
        }
    }

    #[test]
    fn checkin_persists_canonical_blob_when_writable() {
        let store = MemStore::with_entries(&[]);
        let mut blob = ResourceBlob::empty();
        blob.env
            .insert("GH_TOKEN".into(), SecretBytes::from("ghp_new"));
        let canonical = blob.to_canonical_bytes().unwrap();

        let resp = handle_checkin(
            canonical.clone(),
            &store,
            "wrapper",
            "github/token",
            AccessMode::ReadWrite,
        );
        assert!(matches!(resp, WrapperResourceResponse::CheckinOk));

        // Re-checkout sees the new state.
        let resp = handle_checkout(&store, "wrapper", "github/token", AccessMode::ReadWrite);
        match resp {
            WrapperResourceResponse::CheckoutOk { blob: bytes, .. } => {
                assert_eq!(bytes, canonical);
            }
            other => panic!("expected CheckoutOk, got {other:?}"),
        }
    }

    #[test]
    fn checkin_rejects_when_grant_is_readonly() {
        let store = MemStore::with_entries(&[]);
        let canonical = ResourceBlob::empty().to_canonical_bytes().unwrap();
        let resp = handle_checkin(
            canonical,
            &store,
            "wrapper",
            "github/token",
            AccessMode::ReadOnly,
        );
        assert!(matches!(
            resp,
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Unauthorized,
                ..
            }
        ));
    }

    #[test]
    fn checkin_rejects_non_canonical_bytes() {
        let store = MemStore::with_entries(&[]);
        // Hand-crafted JSON with keys in non-canonical order — same
        // shape as the admin PUT noncanonical-blob test.
        let noncanonical =
            br#"{"schema_version":1,"libsecret":{"aliases":{},"collections":{}},"files":{},"env":{}}"#
                .to_vec();
        let resp = handle_checkin(
            noncanonical,
            &store,
            "wrapper",
            "github/token",
            AccessMode::ReadWrite,
        );
        match resp {
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::InvalidRequest,
                message,
            } => {
                assert!(
                    message.contains("noncanonical_blob"),
                    "expected noncanonical_blob, got {message:?}",
                );
            }
            other => panic!("expected InvalidRequest, got {other:?}"),
        }
    }

    #[test]
    fn checkin_rejects_undecodable_bytes() {
        let store = MemStore::with_entries(&[]);
        let resp = handle_checkin(
            b"not json".to_vec(),
            &store,
            "wrapper",
            "github/token",
            AccessMode::ReadWrite,
        );
        assert!(matches!(
            resp,
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::InvalidRequest,
                ..
            }
        ));
    }

    #[test]
    fn usage_signal_is_a_no_op_read() {
        let store = MemStore::with_entries(&[("github/token/env/GH_TOKEN", b"ghp_old")]);
        let resp = handle_usage_signal(&store, "wrapper", "github/token");
        assert!(matches!(resp, WrapperResourceResponse::UsageSignalOk));
    }

    #[test]
    fn usage_signal_succeeds_for_unknown_resource() {
        // A missing blob is indistinguishable from an empty one at this
        // layer — last_used bumping is a backend concern, the channel
        // just signals.
        let store = MemStore::with_entries(&[]);
        let resp = handle_usage_signal(&store, "wrapper", "missing");
        assert!(matches!(resp, WrapperResourceResponse::UsageSignalOk));
    }
}
