//! `POST /v1/wrapper-invocations` (open) and
//! `POST /v1/wrapper-invocations/finalize` (close) — the two
//! stateless calls that bracket a wrapper run. Open returns the
//! resource blob plus a `writable` hint; finalize optionally writes
//! a new blob back (validated against env-immutability and canonical
//! form) and bumps `last_used_at`.

use anyhow::Result;
use axum::Json;
use tracing::warn;

use underwrap_protocol::policy::{PolicyDecision, PolicyDocument, WrapperPermission, decide};
use underwrap_protocol::variant::WrapperVariant;
use underwrap_store::{StorageBackend, load_resource_blob, save_resource_blob};

use crate::runtime_api::error::{IntoInternal, RuntimeError};
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
    variant: WrapperVariant,
    policy: &'a PolicyDocument,
) -> Result<InvocationLookup<'a>> {
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
    // The shared `decide` is the single authority for the (mode × variant)
    // matrix; the daemon-side evaluator and this backend handler must
    // reach identical conclusions. A `Debug` variant is only ever allowed
    // under an active `UnsafeDebugUntil` permission.
    match decide(permission, variant, crate::time::now_epoch_secs()) {
        PolicyDecision::Allow => Ok(InvocationLookup::Allowed {
            resource_id: resource.id.clone(),
            permission,
        }),
        PolicyDecision::Deny { reason } => Ok(InvocationLookup::Denied(reason)),
    }
}

/// Either an Allow tuple `(resource_id, writable)` or a structured
/// Deny reason. Handlers translate the Deny arm into their endpoint's
/// `Deny` response variant.
pub(crate) enum InvocationResolution {
    Allow { resource_id: String, writable: bool },
    Deny(String),
}

/// Run [`resolve_invocation`] and collapse the four-arm enum into the
/// two-arm [`InvocationResolution`]. Any storage error becomes a
/// `RuntimeError::internal`; structural denies (unregistered pair, no
/// permission, denied permission) become `Deny(reason)`.
pub(crate) fn resolve_invocation_or_deny(
    store: &dyn StorageBackend,
    principal: &str,
    wrapper_name: &str,
    resource_name: &str,
    variant: WrapperVariant,
) -> Result<InvocationResolution, RuntimeError> {
    let policy = load_policy_document(store).into_internal("loading policy document")?;
    let lookup = resolve_invocation(
        store,
        principal,
        wrapper_name,
        resource_name,
        variant,
        &policy,
    )
    .into_internal("policy lookup failed")?;
    Ok(match lookup {
        InvocationLookup::UnregisteredPair => {
            InvocationResolution::Deny("unregistered-pair".into())
        }
        InvocationLookup::NoPermission => InvocationResolution::Deny("no-permission".into()),
        InvocationLookup::Denied(reason) => InvocationResolution::Deny(reason),
        InvocationLookup::Allowed {
            resource_id,
            permission,
        } => InvocationResolution::Allow {
            resource_id,
            writable: permission.writable,
        },
    })
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
    let now = crate::time::now_epoch_secs();
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
) -> Result<Json<underwrap_protocol::service::RuntimeWrapperInvocationOpenResponse>, RuntimeError> {
    use base64::Engine;
    use underwrap_protocol::service::RuntimeWrapperInvocationOpenResponse;

    if body.wrapper_name.trim().is_empty() || body.resource_name.trim().is_empty() {
        return Err(RuntimeError::bad_request(
            "invalid_request",
            "wrapper_name and resource_name must be non-empty",
        ));
    }
    let (resource_id, writable) = match resolve_invocation_or_deny(
        store,
        principal,
        &body.wrapper_name,
        &body.resource_name,
        body.variant,
    )? {
        InvocationResolution::Allow {
            resource_id,
            writable,
        } => (resource_id, writable),
        InvocationResolution::Deny(reason) => {
            return Ok(Json(RuntimeWrapperInvocationOpenResponse::Deny { reason }));
        }
    };
    let blob = load_resource_blob(store, &resource_id).into_internal("loading resource blob")?;
    let bytes = blob
        .to_canonical_bytes()
        .into_internal("serializing resource blob")?;
    if let Err(e) = bump_pair_last_used(store, &body.wrapper_name, &resource_id) {
        warn!(
            audit = "pair_last_used_bump_failed",
            principal = %principal,
            wrapper = %body.wrapper_name,
            resource_id = %resource_id,
            err = %e,
        );
    }
    Ok(Json(RuntimeWrapperInvocationOpenResponse::Allow {
        resource_id,
        blob_b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        writable,
    }))
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
) -> Result<Json<underwrap_protocol::service::RuntimeWrapperInvocationFinalizeResponse>, RuntimeError>
{
    use base64::Engine;
    use underwrap_protocol::resource_blob::ResourceBlob;
    use underwrap_protocol::service::RuntimeWrapperInvocationFinalizeResponse;

    if body.wrapper_name.trim().is_empty() || body.resource_name.trim().is_empty() {
        return Err(RuntimeError::bad_request(
            "invalid_request",
            "wrapper_name and resource_name must be non-empty",
        ));
    }
    let (resource_id, writable) = match resolve_invocation_or_deny(
        store,
        principal,
        &body.wrapper_name,
        &body.resource_name,
        body.variant,
    )? {
        InvocationResolution::Allow {
            resource_id,
            writable,
        } => (resource_id, writable),
        InvocationResolution::Deny(reason) => {
            return Ok(Json(RuntimeWrapperInvocationFinalizeResponse::Deny {
                reason,
            }));
        }
    };

    if let Some(blob_b64) = &body.blob_b64 {
        if !writable {
            return Ok(Json(RuntimeWrapperInvocationFinalizeResponse::Deny {
                reason: "not-writable".to_string(),
            }));
        }
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(blob_b64) else {
            return Ok(Json(
                RuntimeWrapperInvocationFinalizeResponse::InvalidBlob {
                    reason: "decode-failed".to_string(),
                },
            ));
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
                return Ok(Json(
                    RuntimeWrapperInvocationFinalizeResponse::InvalidBlob {
                        reason: reason.to_string(),
                    },
                ));
            }
        };
        let canonical = new_blob
            .to_canonical_bytes()
            .into_internal("re-serializing blob")?;
        if canonical != bytes {
            return Ok(Json(
                RuntimeWrapperInvocationFinalizeResponse::InvalidBlob {
                    reason: "noncanonical-blob".to_string(),
                },
            ));
        }
        let current =
            load_resource_blob(store, &resource_id).into_internal("loading current blob")?;
        if new_blob.env != current.env {
            return Ok(Json(
                RuntimeWrapperInvocationFinalizeResponse::InvalidBlob {
                    reason: "env-section-immutable".to_string(),
                },
            ));
        }
        save_resource_blob(store, &resource_id, &new_blob).into_internal("saving resource blob")?;
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
    Ok(Json(RuntimeWrapperInvocationFinalizeResponse::Ok))
}

#[cfg(test)]
mod tests;
