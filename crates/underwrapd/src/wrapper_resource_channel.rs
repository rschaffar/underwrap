//! Wrapper-resource channel handler.
//!
//! Serves the bulk-blob IPC verbs (`Checkout`, `Checkin`,
//! `UsageSignal`) over an authenticated socketpair. The channel is
//! scoped to one wrapper invocation; the daemon has already opened
//! the invocation against the backend (see `wrapper_flow::read`) and
//! handed the canonical resource blob bytes to this handler.
//!
//! Persistence semantics:
//!
//! - `Checkout` returns the cached blob bytes verbatim and the
//!   backend-supplied `writable` hint.
//! - `Checkin` issues `POST /v1/wrapper-invocations/finalize` with
//!   the wrapper-supplied blob. The backend re-checks writability,
//!   re-canonicalizes, and rejects any change to the env section.
//! - `UsageSignal` issues `POST /v1/wrapper-invocations/finalize`
//!   with no blob — bumps `last_used_at` server-side without writing.

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::UnixStream;
use tracing::{debug, error, info, warn};

use underwrap_protocol::config::AccessMode;
use underwrap_protocol::ipc::{
    read_wrapper_resource_frame_async, write_wrapper_resource_frame_async,
};
use underwrap_protocol::message::{
    ResourceErrorCode, WrapperResourceRequest, WrapperResourceResponse,
};
use underwrap_protocol::service::RuntimeWrapperInvocationFinalizeResponse;
use underwrap_protocol::variant::WrapperVariant;

use crate::backend_client::BackendClient;

/// State carried for the lifetime of one channel.
pub struct ChannelContext {
    /// Client-supplied (and validated) wrapper name used both in
    /// logs/audit and in backend API calls.
    pub wrapper_name: String,
    /// Invocation variant; re-sent on every finalize so the backend
    /// re-evaluates the same policy branch it allowed on open.
    pub variant: WrapperVariant,
    /// Resource name used in backend API calls.
    pub resource_name: String,
    /// Opaque backend-supplied resource id. The daemon never addresses
    /// storage by it; it is used in channel logs and relayed to the
    /// wrapper, where `underwrap-ns` surfaces it in audit events and the
    /// user-facing writeback-rejected message.
    pub resource_id: String,
    /// Backend-supplied writable hint captured at open time. Surfaced
    /// to the wrapper on `Checkout` so it can fail closed early; the
    /// authoritative re-check happens on the backend at finalize.
    pub access: AccessMode,
    /// Canonical resource blob returned by the backend on open. Served
    /// verbatim on `Checkout`.
    pub cached_blob: Vec<u8>,
}

pub async fn run_channel(stream: UnixStream, backend: Arc<BackendClient>, ctx: ChannelContext) {
    info!(
        wrapper = %ctx.wrapper_name,
        resource = %ctx.resource_id,
        access = ?ctx.access,
        "wrapper-resource channel opened",
    );
    if let Err(e) = channel_loop(stream, backend, &ctx).await {
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
    backend: Arc<BackendClient>,
    ctx: &ChannelContext,
) -> Result<()> {
    loop {
        let req: WrapperResourceRequest = match read_wrapper_resource_frame_async(&mut stream).await
        {
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
        let backend = Arc::clone(&backend);
        let ctx_wrapper = ctx.wrapper_name.clone();
        let ctx_variant = ctx.variant;
        let ctx_resource_name = ctx.resource_name.clone();
        let ctx_resource_id = ctx.resource_id.clone();
        let ctx_access = ctx.access;
        let cached = ctx.cached_blob.clone();
        // The BackendClient API is sync (it owns its own runtime
        // thread), so wrap the call in `spawn_blocking` to avoid
        // stalling the tokio worker that owns this channel.
        let resp = tokio::task::spawn_blocking(move || {
            handle_request(
                req,
                &backend,
                &ctx_wrapper,
                ctx_variant,
                &ctx_resource_name,
                &ctx_resource_id,
                ctx_access,
                &cached,
            )
        })
        .await
        .unwrap_or_else(|join_err| WrapperResourceResponse::Error {
            code: ResourceErrorCode::Internal,
            message: format!("channel handler panicked: {join_err}"),
        });
        write_wrapper_resource_frame_async(&mut stream, &resp).await?;
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_request(
    request: WrapperResourceRequest,
    backend: &BackendClient,
    wrapper_name: &str,
    variant: WrapperVariant,
    resource_name: &str,
    resource_id: &str,
    access: AccessMode,
    cached_blob: &[u8],
) -> WrapperResourceResponse {
    match request {
        WrapperResourceRequest::Checkout => {
            handle_checkout(wrapper_name, resource_id, access, cached_blob)
        }
        WrapperResourceRequest::Checkin { blob } => handle_checkin(
            blob,
            backend,
            wrapper_name,
            variant,
            resource_name,
            resource_id,
        ),
        WrapperResourceRequest::UsageSignal => {
            handle_usage_signal(backend, wrapper_name, variant, resource_name, resource_id)
        }
    }
}

fn handle_checkout(
    wrapper_name: &str,
    resource_id: &str,
    access: AccessMode,
    cached_blob: &[u8],
) -> WrapperResourceResponse {
    debug!(
        wrapper = %wrapper_name,
        resource = %resource_id,
        bytes = cached_blob.len(),
        "wrapper-resource checkout",
    );
    crate::audit::resource_checkout(wrapper_name, resource_id);
    WrapperResourceResponse::CheckoutOk {
        blob: cached_blob.to_vec(),
        writable: access == AccessMode::ReadWrite,
    }
}

fn handle_checkin(
    blob: Vec<u8>,
    backend: &BackendClient,
    wrapper_name: &str,
    variant: WrapperVariant,
    resource_name: &str,
    resource_id: &str,
) -> WrapperResourceResponse {
    match backend.finalize_invocation(wrapper_name, resource_name, variant, Some(&blob)) {
        Ok(RuntimeWrapperInvocationFinalizeResponse::Ok) => {
            debug!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                "wrapper-resource checkin",
            );
            crate::audit::resource_checkin(wrapper_name, resource_id);
            WrapperResourceResponse::CheckinOk
        }
        Ok(RuntimeWrapperInvocationFinalizeResponse::Deny { reason }) => {
            crate::audit::wrapper_write_denied(
                "resource",
                wrapper_name,
                resource_id,
                "<full-blob>",
                &reason,
            );
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Unauthorized,
                message: reason,
            }
        }
        Ok(RuntimeWrapperInvocationFinalizeResponse::InvalidBlob { reason }) => {
            warn!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                reason = %reason,
                "backend rejected blob on checkin",
            );
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::InvalidRequest,
                message: reason,
            }
        }
        Err(e) => {
            error!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                err = %format!("{e:#}"),
                "backend finalize failed on checkin",
            );
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Internal,
                message: "backend finalize failed".to_string(),
            }
        }
    }
}

fn handle_usage_signal(
    backend: &BackendClient,
    wrapper_name: &str,
    variant: WrapperVariant,
    resource_name: &str,
    resource_id: &str,
) -> WrapperResourceResponse {
    match backend.finalize_invocation(wrapper_name, resource_name, variant, None) {
        Ok(RuntimeWrapperInvocationFinalizeResponse::Ok) => {
            debug!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                "wrapper-resource usage signal",
            );
            crate::audit::resource_usage_signal(wrapper_name, resource_id);
            WrapperResourceResponse::UsageSignalOk
        }
        Ok(RuntimeWrapperInvocationFinalizeResponse::Deny { reason }) => {
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Unauthorized,
                message: reason,
            }
        }
        // InvalidBlob is impossible on a None-blob finalize, but
        // surface it conservatively as Internal rather than panic if
        // the backend ever returns it.
        Ok(RuntimeWrapperInvocationFinalizeResponse::InvalidBlob { reason }) => {
            error!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                reason = %reason,
                "backend returned InvalidBlob on a no-blob usage-signal finalize",
            );
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Internal,
                message: "backend rejected usage signal".to_string(),
            }
        }
        Err(e) => {
            error!(
                wrapper = %wrapper_name,
                resource = %resource_id,
                err = %format!("{e:#}"),
                "backend finalize failed on usage signal",
            );
            WrapperResourceResponse::Error {
                code: ResourceErrorCode::Internal,
                message: "usage signal failed".to_string(),
            }
        }
    }
}
