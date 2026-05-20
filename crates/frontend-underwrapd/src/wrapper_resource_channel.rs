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

/// Read-time view of the channel that the handler retains for the
/// lifetime of the connection. `access` is captured at channel setup
/// and gates Checkout's `writable` flag; Checkin additionally
/// re-evaluates the policy document on every call so an operator who
/// flips a grant from writable to read-only mid-run is observed before
/// the write hits the backend.
pub struct ChannelContext {
    pub wrapper_name: String,
    pub resource_id: String,
    pub access: AccessMode,
    /// Client instance id the policy is keyed on.
    pub client_instance: String,
    /// Logical wrapper name used by the policy lookup (matches what
    /// the read flow consults).
    pub logical_name: String,
}

/// Run the wrapper-resource channel handler loop on the daemon end of
/// the socketpair. Returns when the wrapper end closes or a fatal
/// protocol error occurs.
pub async fn run_channel(stream: UnixStream, store: Arc<dyn StorageBackend>, ctx: ChannelContext) {
    info!(
        wrapper = %ctx.wrapper_name,
        resource = %ctx.resource_id,
        access = ?ctx.access,
        "wrapper-resource channel opened",
    );
    if let Err(e) = channel_loop(stream, &*store, &ctx).await {
        debug!(
            wrapper = %ctx.wrapper_name,
            resource = %ctx.resource_id,
            err = %e,
            "wrapper-resource channel closed",
        );
    }
    info!(
        wrapper = %ctx.wrapper_name,
        resource = %ctx.resource_id,
        "wrapper-resource channel handler exited",
    );
}

async fn channel_loop(
    mut stream: UnixStream,
    store: &dyn StorageBackend,
    ctx: &ChannelContext,
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
        let resp = handle_request(req, store, ctx);
        write_frame_async(&mut stream, &resp).await?;
    }
}

fn handle_request(
    request: WrapperResourceRequest,
    store: &dyn StorageBackend,
    ctx: &ChannelContext,
) -> WrapperResourceResponse {
    match request {
        WrapperResourceRequest::Checkout => {
            handle_checkout(store, &ctx.wrapper_name, &ctx.resource_id, ctx.access)
        }
        WrapperResourceRequest::Checkin { blob } => handle_checkin(blob, store, ctx),
        WrapperResourceRequest::UsageSignal => {
            handle_usage_signal(store, &ctx.wrapper_name, &ctx.resource_id)
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
    ctx: &ChannelContext,
) -> WrapperResourceResponse {
    let wrapper_name = ctx.wrapper_name.as_str();
    let resource_id = ctx.resource_id.as_str();

    if ctx.access != AccessMode::ReadWrite {
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

    // Re-evaluate the policy at Checkin time so an operator who
    // revokes write access mid-run is observed before the write
    // touches the backend. The access mode captured at channel
    // setup is the floor, not the ceiling.
    match crate::policy::load_policy_document(store) {
        Ok(doc) => {
            if !crate::policy::writable_for(
                doc.as_ref(),
                &ctx.client_instance,
                &ctx.logical_name,
                resource_id,
            ) {
                crate::audit::wrapper_write_denied(
                    "resource",
                    wrapper_name,
                    resource_id,
                    "<full-blob>",
                    "grant was flipped to read-only between Checkout and Checkin",
                );
                return WrapperResourceResponse::Error {
                    code: ResourceErrorCode::Unauthorized,
                    message: "resource grant is no longer writable".to_string(),
                };
            }
        }
        Err(e) => {
            error!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                err = %e,
                "loading policy document on checkin",
            );
            return WrapperResourceResponse::Error {
                code: ResourceErrorCode::Internal,
                message: "loading policy document failed".to_string(),
            };
        }
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

    fn ctx_for(access: AccessMode) -> ChannelContext {
        ChannelContext {
            wrapper_name: "wrapper".to_string(),
            resource_id: "github/token".to_string(),
            access,
            client_instance: "dev-laptop".to_string(),
            logical_name: "wrapper".to_string(),
        }
    }

    /// Build a policy document with one writable, unlocked permission
    /// for the canonical test tuple `(dev-laptop, wrapper, github/token)`.
    /// The deny-by-default gate requires every reachable test scenario
    /// to declare its permission explicitly.
    fn writable_policy() -> underwrap_protocol::policy::PolicyDocument {
        use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
        PolicyDocument {
            permissions: vec![WrapperPermission {
                client_instance: "dev-laptop".into(),
                wrapper: "wrapper".into(),
                resource_id: "github/token".into(),
                mode: WrapperApprovalMode::Unlocked,
                writable: true,
                reason: None,
            }],
        }
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
        let store = MemStore::with_entries_and_policy(&[], writable_policy());
        let mut blob = ResourceBlob::empty();
        blob.env
            .insert("GH_TOKEN".into(), SecretBytes::from("ghp_new"));
        let canonical = blob.to_canonical_bytes().unwrap();

        let resp = handle_checkin(canonical.clone(), &store, &ctx_for(AccessMode::ReadWrite));
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
        let resp = handle_checkin(canonical, &store, &ctx_for(AccessMode::ReadOnly));
        assert!(matches!(
            resp,
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Unauthorized,
                ..
            }
        ));
    }

    #[test]
    fn checkin_rejects_when_policy_flipped_readonly_mid_run() {
        use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
        let policy = PolicyDocument {
            permissions: vec![WrapperPermission {
                client_instance: "dev-laptop".into(),
                wrapper: "wrapper".into(),
                resource_id: "github/token".into(),
                mode: WrapperApprovalMode::Unlocked,
                writable: false,
                reason: None,
            }],
        };
        let store = MemStore::with_entries_and_policy(&[], policy);
        let canonical = ResourceBlob::empty().to_canonical_bytes().unwrap();
        // Channel was opened with writable=true, but the live policy
        // now says writable=false. The mid-run re-check must reject.
        let resp = handle_checkin(canonical, &store, &ctx_for(AccessMode::ReadWrite));
        match resp {
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Unauthorized,
                message,
            } => assert!(
                message.contains("no longer writable"),
                "expected mid-run rejection, got {message:?}",
            ),
            other => panic!("expected Unauthorized, got {other:?}"),
        }
    }

    #[test]
    fn checkin_rejects_non_canonical_bytes() {
        let store = MemStore::with_entries_and_policy(&[], writable_policy());
        // Hand-crafted JSON with keys in non-canonical order — same
        // shape as the admin PUT noncanonical-blob test.
        let noncanonical =
            br#"{"schema_version":1,"libsecret":{"aliases":{},"collections":{}},"files":{},"env":{}}"#
                .to_vec();
        let resp = handle_checkin(noncanonical, &store, &ctx_for(AccessMode::ReadWrite));
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
        let store = MemStore::with_entries_and_policy(&[], writable_policy());
        let resp = handle_checkin(
            b"not json".to_vec(),
            &store,
            &ctx_for(AccessMode::ReadWrite),
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
    fn checkin_rejects_unknown_schema_version() {
        // Hand-crafted JCS-canonical bytes (keys sorted lexicographically)
        // for an otherwise valid ResourceBlob carrying an unsupported
        // schema version. ResourceBlob::from_canonical_bytes refuses
        // unknown versions, so the channel surfaces InvalidRequest.
        let store = MemStore::with_entries_and_policy(&[], writable_policy());
        let bytes =
            br#"{"env":{},"files":{},"libsecret":{"aliases":{},"collections":{}},"schema_version":999}"#
                .to_vec();
        let resp = handle_checkin(bytes, &store, &ctx_for(AccessMode::ReadWrite));
        match resp {
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::InvalidRequest,
                ..
            } => {}
            other => panic!("expected InvalidRequest on unknown schema, got {other:?}"),
        }
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
