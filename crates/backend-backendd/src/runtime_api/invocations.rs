//! `POST /v1/wrapper-invocations` (open) and
//! `POST /v1/wrapper-invocations/finalize` (close) — the two
//! stateless calls that bracket a wrapper run. Open returns the
//! resource blob plus a `writable` hint; finalize optionally writes
//! a new blob back (validated against env-immutability and canonical
//! form) and bumps `last_used_at`.

use anyhow::Result;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};
use tracing::warn;

use underwrap_protocol::policy::{PolicyDocument, WrapperPermission};
use underwrap_store::{StorageBackend, load_resource_blob, save_resource_blob};

use crate::http::responses::{error_response, json_response};
use crate::storage::documents::{
    load_pairs_document, load_policy_document, load_resources_document, save_pairs_document,
};

/// Result of the policy + pair lookup shared by open and finalize.
///
/// Encodes the four authoritative outcomes the backend draws from the
/// per-store state: pair not registered, no permission at all, the
/// permission denies right now, or the permission allows.
pub(crate) enum InvocationLookup<'a> {
    UnregisteredPair,
    NoPermission,
    Denied(String),
    Allowed {
        resource_id: String,
        permission: &'a WrapperPermission,
    },
}

/// Resolve `(principal, wrapper_name, resource_name)` to either an
/// allow-with-permission or a structured denial. Used by the open and
/// finalize handlers; both must reach the same conclusion from the
/// same inputs (stateless re-evaluation per the redesign).
pub(crate) fn resolve_invocation<'a>(
    store: &dyn StorageBackend,
    principal: &str,
    wrapper_name: &str,
    resource_name: &str,
    policy: &'a PolicyDocument,
) -> Result<InvocationLookup<'a>> {
    use underwrap_protocol::policy::WrapperApprovalMode;

    let resources = load_resources_document(store)?;
    let Some(resource) = resources.find_by_name(resource_name) else {
        return Ok(InvocationLookup::UnregisteredPair);
    };
    let pairs = load_pairs_document(store)?;
    if pairs.find(wrapper_name, &resource.id).is_none() {
        return Ok(InvocationLookup::UnregisteredPair);
    }
    let Some(permission) = policy.find_permission(principal, wrapper_name, &resource.id) else {
        return Ok(InvocationLookup::NoPermission);
    };
    match &permission.mode {
        WrapperApprovalMode::Locked => Ok(InvocationLookup::Denied(
            permission
                .reason
                .clone()
                .unwrap_or_else(|| "locked".to_string()),
        )),
        WrapperApprovalMode::Unlocked => Ok(InvocationLookup::Allowed {
            resource_id: resource.id.clone(),
            permission,
        }),
        WrapperApprovalMode::UnlockedUntil { until_epoch } => {
            if crate::users::now_epoch_secs() > *until_epoch {
                Ok(InvocationLookup::Denied("expired".to_string()))
            } else {
                Ok(InvocationLookup::Allowed {
                    resource_id: resource.id.clone(),
                    permission,
                })
            }
        }
    }
}

/// Set `last_used_at` on the `(wrapper, resource_id)` pair to the
/// current wall-clock second. Idempotent at one-second resolution:
/// the pairs document is only rewritten if the new timestamp differs
/// from the stored one.
pub(crate) fn bump_pair_last_used(
    store: &dyn StorageBackend,
    wrapper: &str,
    resource_id: &str,
) -> Result<()> {
    let now = crate::users::now_epoch_secs();
    let mut doc = load_pairs_document(store)?;
    let Some(pair) = doc
        .pairs
        .iter_mut()
        .find(|p| p.wrapper == wrapper && p.resource_id == resource_id)
    else {
        // No pair record means nothing to bump. This can happen if
        // the pair was deleted while a wrapper invocation was in
        // flight, or in the greenfield case where no entities have
        // been created yet.
        return Ok(());
    };
    if pair.last_used_at == Some(now) {
        return Ok(());
    }
    pair.last_used_at = Some(now);
    save_pairs_document(store, &doc)
}

/// Handle `POST /v1/wrapper-invocations` — open an invocation.
pub(crate) fn handle_runtime_wrapper_invocation_open(
    store: &dyn StorageBackend,
    principal: &str,
    body: &underwrap_protocol::service::RuntimeWrapperInvocationOpenRequest,
) -> Response<Full<Bytes>> {
    use base64::Engine;
    use underwrap_protocol::service::RuntimeWrapperInvocationOpenResponse;

    if body.wrapper_name.trim().is_empty() || body.resource_name.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "wrapper_name and resource_name must be non-empty",
        );
    }
    let policy = match load_policy_document(store) {
        Ok(p) => p,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading policy: {e:#}"),
            );
        }
    };
    let lookup = match resolve_invocation(
        store,
        principal,
        &body.wrapper_name,
        &body.resource_name,
        &policy,
    ) {
        Ok(l) => l,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("policy lookup failed: {e:#}"),
            );
        }
    };
    let (resource_id, writable) = match lookup {
        InvocationLookup::UnregisteredPair => {
            return json_response(
                StatusCode::OK,
                &RuntimeWrapperInvocationOpenResponse::Deny {
                    reason: "unregistered-pair".to_string(),
                },
            );
        }
        InvocationLookup::NoPermission => {
            return json_response(
                StatusCode::OK,
                &RuntimeWrapperInvocationOpenResponse::Deny {
                    reason: "no-permission".to_string(),
                },
            );
        }
        InvocationLookup::Denied(reason) => {
            return json_response(
                StatusCode::OK,
                &RuntimeWrapperInvocationOpenResponse::Deny { reason },
            );
        }
        InvocationLookup::Allowed {
            resource_id,
            permission,
        } => (resource_id, permission.writable),
    };
    let blob = match load_resource_blob(store, &resource_id) {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading resource blob: {e:#}"),
            );
        }
    };
    let bytes = match blob.to_canonical_bytes() {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("serializing resource blob: {e:#}"),
            );
        }
    };
    if let Err(e) = bump_pair_last_used(store, &body.wrapper_name, &resource_id) {
        warn!(
            audit = "pair_last_used_bump_failed",
            principal = %principal,
            wrapper = %body.wrapper_name,
            resource_id = %resource_id,
            err = %e,
        );
    }
    json_response(
        StatusCode::OK,
        &RuntimeWrapperInvocationOpenResponse::Allow {
            resource_id,
            blob_b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
            writable,
        },
    )
}

/// Handle `POST /v1/wrapper-invocations/finalize` — close an invocation.
///
/// Stateless: no prior open call is required. If `blob_b64` is `None`
/// this is a usage-signal-only finalize that bumps `last_used_at`.
/// If `blob_b64` is `Some`, the backend independently re-checks
/// writability, decodes the blob, requires canonical bytes, and
/// rejects any change to the env section.
pub(crate) fn handle_runtime_wrapper_invocation_finalize(
    store: &dyn StorageBackend,
    principal: &str,
    body: &underwrap_protocol::service::RuntimeWrapperInvocationFinalizeRequest,
) -> Response<Full<Bytes>> {
    use base64::Engine;
    use underwrap_protocol::resource_blob::ResourceBlob;
    use underwrap_protocol::service::RuntimeWrapperInvocationFinalizeResponse;

    if body.wrapper_name.trim().is_empty() || body.resource_name.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "wrapper_name and resource_name must be non-empty",
        );
    }
    let policy = match load_policy_document(store) {
        Ok(p) => p,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading policy: {e:#}"),
            );
        }
    };
    let lookup = match resolve_invocation(
        store,
        principal,
        &body.wrapper_name,
        &body.resource_name,
        &policy,
    ) {
        Ok(l) => l,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("policy lookup failed: {e:#}"),
            );
        }
    };
    let (resource_id, writable) = match lookup {
        InvocationLookup::UnregisteredPair => {
            return json_response(
                StatusCode::OK,
                &RuntimeWrapperInvocationFinalizeResponse::Deny {
                    reason: "unregistered-pair".to_string(),
                },
            );
        }
        InvocationLookup::NoPermission => {
            return json_response(
                StatusCode::OK,
                &RuntimeWrapperInvocationFinalizeResponse::Deny {
                    reason: "no-permission".to_string(),
                },
            );
        }
        InvocationLookup::Denied(reason) => {
            return json_response(
                StatusCode::OK,
                &RuntimeWrapperInvocationFinalizeResponse::Deny { reason },
            );
        }
        InvocationLookup::Allowed {
            resource_id,
            permission,
        } => (resource_id, permission.writable),
    };

    if let Some(blob_b64) = &body.blob_b64 {
        if !writable {
            return json_response(
                StatusCode::OK,
                &RuntimeWrapperInvocationFinalizeResponse::Deny {
                    reason: "not-writable".to_string(),
                },
            );
        }
        let bytes = match base64::engine::general_purpose::STANDARD.decode(blob_b64) {
            Ok(b) => b,
            Err(_) => {
                return json_response(
                    StatusCode::OK,
                    &RuntimeWrapperInvocationFinalizeResponse::InvalidBlob {
                        reason: "decode-failed".to_string(),
                    },
                );
            }
        };
        let new_blob = match ResourceBlob::from_canonical_bytes(&bytes) {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("{e:#}");
                // `from_canonical_bytes` rejects unknown schema versions
                // with a message starting with `unsupported resource
                // blob schema_version`; everything else is a JSON
                // decode failure. The wire reason distinguishes them
                // so the daemon can give the operator a useful error.
                let reason = if msg.contains("schema_version") {
                    "schema-unknown"
                } else {
                    "decode-failed"
                };
                return json_response(
                    StatusCode::OK,
                    &RuntimeWrapperInvocationFinalizeResponse::InvalidBlob {
                        reason: reason.to_string(),
                    },
                );
            }
        };
        let canonical = match new_blob.to_canonical_bytes() {
            Ok(b) => b,
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &format!("re-serializing blob: {e:#}"),
                );
            }
        };
        if canonical != bytes {
            return json_response(
                StatusCode::OK,
                &RuntimeWrapperInvocationFinalizeResponse::InvalidBlob {
                    reason: "noncanonical-blob".to_string(),
                },
            );
        }
        let current = match load_resource_blob(store, &resource_id) {
            Ok(b) => b,
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &format!("loading current blob: {e:#}"),
                );
            }
        };
        if new_blob.env != current.env {
            return json_response(
                StatusCode::OK,
                &RuntimeWrapperInvocationFinalizeResponse::InvalidBlob {
                    reason: "env-section-immutable".to_string(),
                },
            );
        }
        if let Err(e) = save_resource_blob(store, &resource_id, &new_blob) {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("saving resource blob: {e:#}"),
            );
        }
    }

    if let Err(e) = bump_pair_last_used(store, &body.wrapper_name, &resource_id) {
        warn!(
            audit = "pair_last_used_bump_failed",
            principal = %principal,
            wrapper = %body.wrapper_name,
            resource_id = %resource_id,
            err = %e,
        );
    }
    json_response(
        StatusCode::OK,
        &RuntimeWrapperInvocationFinalizeResponse::Ok,
    )
}

#[cfg(test)]
mod tests;
