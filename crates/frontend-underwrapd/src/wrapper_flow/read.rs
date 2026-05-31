//! Open one invocation against the backend and build the wrapper-side
//! `ReadOk` response.
//!
//! The daemon delegates every authorization decision to the backend's
//! `POST /v1/wrapper-invocations` endpoint. On Allow the backend
//! returns the canonical resource blob and a `writable` hint; the
//! daemon extracts the env section (when the wrapper config opts into
//! it), validates env names, and assembles the wire response. The
//! resource blob and the writable hint travel onward to the
//! scoped wrapper-resource channel.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use tracing::{error, warn};

use underwrap_protocol::config::{AccessMode, RESERVED_WRAPPER_NAME_DEBUG};
use underwrap_protocol::message::{ErrorCode, SecretBytes, WrapperResponse};
use underwrap_protocol::resource::validate_env_name;
use underwrap_protocol::resource_blob::ResourceBlob;
use underwrap_protocol::service::RuntimeWrapperInvocationOpenResponse;
use underwrap_protocol::variant::WrapperVariant;

use crate::audit;
use crate::backend_client::{BackendClient, StoreLockedError, UnauthorizedError};
use crate::server::State;
use crate::wrapper_bindings::wrapper_file_paths;

use super::ReadOutcome;

/// Per-invocation state the channel handler needs after the daemon's
/// `ReadOk` has been written to the wrapper socket.
pub(super) struct OpenedInvocation {
    pub backend: Arc<BackendClient>,
    /// Wrapper name used in `POST /v1/wrapper-invocations` calls. This is
    /// the client-supplied name validated against the daemon config.
    pub wrapper_name: String,
    /// Invocation variant (Real/Debug); re-sent on finalize so the
    /// backend re-evaluates the same policy branch it allowed on open.
    pub variant: WrapperVariant,
    /// Resource name (operator-facing) used in `POST /v1/wrapper-invocations`.
    pub resource_name: String,
    /// Canonical resource blob bytes returned by the backend on open.
    /// The channel serves this verbatim on `Checkout`.
    pub blob_bytes: Vec<u8>,
    /// Backend's writable hint. The channel uses it for the
    /// `CheckoutOk { writable }` field; the backend re-checks on
    /// finalize regardless.
    pub access: AccessMode,
    /// Opaque backend resource id, threaded through to the channel. The
    /// daemon never addresses storage by it, but it is surfaced to the
    /// wrapper (and onward into `frontend-ns` audit events and the
    /// user-facing writeback-rejected message), not just logged.
    pub resource_id: String,
}

pub(super) fn open_invocation(
    _scope: Option<&str>,
    wrapper_name: &str,
    variant: WrapperVariant,
    peer_uid: u32,
    state: &State,
    backend: Arc<BackendClient>,
) -> ReadOutcome {
    // The reserved `debug` token is the frontend-exec subcommand keyword,
    // never a wrapper name. A config can't define it (rejected at load),
    // but reject it explicitly here too so a hand-crafted client gets a
    // clear error rather than a generic "unknown wrapper".
    if wrapper_name == RESERVED_WRAPPER_NAME_DEBUG {
        warn!(wrapper = %wrapper_name, "rejected reserved wrapper name");
        return error_outcome(
            ErrorCode::NotFound,
            format!("'{RESERVED_WRAPPER_NAME_DEBUG}' is a reserved subcommand, not a wrapper"),
        );
    }
    let wrapper_def = match state.config.wrappers.get(wrapper_name) {
        Some(w) => w,
        None => {
            warn!(wrapper = %wrapper_name, "unknown wrapper");
            return error_outcome(
                ErrorCode::NotFound,
                format!("unknown wrapper '{wrapper_name}'"),
            );
        }
    };
    let resource_name = wrapper_def.resource.clone();

    let response = match backend.open_invocation(wrapper_name, &resource_name, variant) {
        Ok(r) => r,
        Err(e) => {
            if let Some(locked) = e.downcast_ref::<StoreLockedError>() {
                error!(wrapper = %wrapper_name, resource = %resource_name, "backing store is locked");
                return error_outcome(ErrorCode::StoreLocked, locked.message.clone());
            }
            if e.downcast_ref::<UnauthorizedError>().is_some() {
                warn!(
                    wrapper = %wrapper_name,
                    err = %format!("{e:#}"),
                    "backend refused this client's credential; surfacing enrollment guidance",
                );
                return error_outcome(
                    ErrorCode::Unauthorized,
                    crate::server::unenrolled_message(&state.enrollment_url()),
                );
            }
            error!(
                wrapper = %wrapper_name,
                resource = %resource_name,
                err = %format!("{e:#}"),
                "backend open_invocation failed",
            );
            return error_outcome(
                ErrorCode::Internal,
                "backend open_invocation failed".to_string(),
            );
        }
    };

    let (resource_id, blob_b64, writable) = match response {
        RuntimeWrapperInvocationOpenResponse::Allow {
            resource_id,
            blob_b64,
            writable,
        } => (resource_id, blob_b64, writable),
        RuntimeWrapperInvocationOpenResponse::Deny { reason } => {
            audit::read_denied(wrapper_name, &reason, Some(peer_uid), None);
            return error_outcome(ErrorCode::Unauthorized, reason);
        }
    };

    let blob_bytes = match base64::engine::general_purpose::STANDARD.decode(&blob_b64) {
        Ok(b) => b,
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "backend returned undecodable blob_b64");
            return error_outcome(
                ErrorCode::Internal,
                "backend returned invalid blob".to_string(),
            );
        }
    };

    let blob = match ResourceBlob::from_canonical_bytes(&blob_bytes) {
        Ok(b) => b,
        Err(e) => {
            error!(wrapper = %wrapper_name, err = %e, "backend returned unparseable blob");
            return error_outcome(
                ErrorCode::Internal,
                "backend returned malformed blob".to_string(),
            );
        }
    };

    let mut env: HashMap<String, SecretBytes> = HashMap::new();
    if wrapper_def.env {
        for (name, value) in &blob.env {
            if let Err(reason) = validate_env_name(name) {
                return error_outcome(
                    ErrorCode::Internal,
                    format!(
                        "resource '{resource_name}' contains invalid env name '{name}' for wrapper '{wrapper_name}': {reason}"
                    ),
                );
            }
            env.insert(name.clone(), value.clone());
        }
    }

    audit::read_granted(wrapper_name, &resource_id, Some(peer_uid), None);

    let access = if writable {
        AccessMode::ReadWrite
    } else {
        AccessMode::ReadOnly
    };
    let files_target_paths = wrapper_file_paths(wrapper_def);
    let libsecret = wrapper_def.libsecret;

    // The daemon — not the wrapper — chooses the binary to exec. For a
    // `Real` invocation it is the wrapper's configured target; for a
    // `Debug` invocation (only reachable after the backend allowed it
    // above) it is the instance's debug tool. A client that forges
    // `variant = Real` can never reach the debug tool, and one that
    // forges `variant = Debug` is rejected by the backend policy unless
    // the permission is in `UnsafeDebugUntil` mode.
    let target = match variant {
        WrapperVariant::Real => wrapper_def.target.clone(),
        WrapperVariant::Debug => state.config.debug_tool_target.clone(),
    };

    let response = WrapperResponse::ReadOk {
        env,
        resource_id: resource_id.clone(),
        resource_name: resource_name.clone(),
        access,
        libsecret,
        files_target_paths,
        target,
        env_passthrough: wrapper_def.env_passthrough.clone(),
        env_set: wrapper_def.env_set.clone(),
        xdg_runtime_dir_base: state.config.xdg_runtime_dir_base.clone(),
    };
    ReadOutcome {
        response,
        opened: Some(OpenedInvocation {
            backend,
            wrapper_name: wrapper_name.to_string(),
            variant,
            resource_name,
            blob_bytes,
            access,
            resource_id,
        }),
    }
}

fn error_outcome(code: ErrorCode, message: String) -> ReadOutcome {
    ReadOutcome {
        response: WrapperResponse::Error { code, message },
        opened: None,
    }
}
