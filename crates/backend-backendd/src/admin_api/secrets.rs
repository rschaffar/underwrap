//! Per-key admin secret CRUD: `/v1/admin/secret/{read, write, list,
//! delete, rename}`.
//!
//! Translates structured `(resource_id, slot, key)` tuples through the
//! resource-blob layout for per-key reads/writes; the `rename` handler
//! is the only multi-step one and is intentionally a single
//! load-modify-save against the blob so the file flip stays atomic.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::routing::post;
use base64::Engine;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource::ResourceSlot;
use underwrap_store::{
    blob_from_resource_entries, expand_blob_to_entries, load_resource_blob, save_resource_blob,
};

use crate::admin_api::audit::log_admin_event;
use crate::admin_api::stores::admin_store;
use crate::http::responses::{error_response, json_response, parse_body};
use crate::runtime_state::AdminRuntimeState;
use crate::storage::secret_entries::{
    delete_resource_entry, list_secret_entries, read_resource_entry,
    validate_and_compose_secret_id, write_resource_entry,
};
use crate::users::Principal;

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_admin_secret_audit(
    operator: &str,
    store: &str,
    resource_id: &str,
    slot: ResourceSlot,
    key: &str,
    action: &str,
    operation: Option<&str>,
    result: &str,
) {
    let resource = format!("secret:{store}/{resource_id}/{}/{key}", slot.name());
    log_admin_event(
        action,
        operator,
        None,
        Some(&resource),
        None,
        operation,
        result,
    );
}

/// Audit a secret rename. Carries both keys because the operation
/// changes the canonical identifier itself; the formatted resource
/// uses the `from` key so the event lines up with the prior
/// `secret.write` that created it. The new key travels in the
/// `operation` field as `rename->{to_key}`.
pub(crate) fn write_admin_secret_rename_audit(
    operator: &str,
    store: &str,
    resource_id: &str,
    slot: ResourceSlot,
    from_key: &str,
    to_key: &str,
    result: &str,
) {
    let resource = format!("secret:{store}/{resource_id}/{}/{from_key}", slot.name());
    let operation = format!("rename->{to_key}");
    log_admin_event(
        "secret.rename",
        operator,
        None,
        Some(&resource),
        None,
        Some(&operation),
        result,
    );
}

pub(crate) fn handle_admin_secret_read(
    state: &AdminRuntimeState,
    body: &underwrap_protocol::service::AdminSecretReadRequest,
) -> Response<Full<Bytes>> {
    let secret_id = match validate_and_compose_secret_id(&body.resource_id, body.slot, &body.key) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    match admin_store(state, &body.store) {
        Ok(store) => match read_resource_entry(store.as_ref(), &secret_id) {
            Ok(material) => json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminSecretReadResponse {
                    material_b64: material.map(|material| {
                        base64::engine::general_purpose::STANDARD.encode(material.as_ref())
                    }),
                },
            ),
            Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
        },
        Err(resp) => resp,
    }
}

pub(crate) fn handle_admin_secret_list(
    state: &AdminRuntimeState,
    body: &underwrap_protocol::service::AdminSecretListRequest,
) -> Response<Full<Bytes>> {
    match admin_store(state, &body.store) {
        Ok(store) => match list_secret_entries(store.as_ref(), body.resource_id.as_deref()) {
            Ok(entries) => json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminSecretListResponse { entries },
            ),
            Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
        },
        Err(resp) => resp,
    }
}

pub(crate) fn handle_admin_secret_delete(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: &underwrap_protocol::service::AdminSecretDeleteRequest,
) -> Response<Full<Bytes>> {
    // Admin deletes are always allowed once per-store access
    // has been checked; per-resource write flags only gate
    // wrapper-side writes.
    let secret_id = match validate_and_compose_secret_id(&body.resource_id, body.slot, &body.key) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let store = match admin_store(state, &body.store) {
        Ok(store) => store,
        Err(resp) => return resp,
    };
    match delete_resource_entry(store.as_ref(), &secret_id) {
        Ok(existed) => {
            write_admin_secret_audit(
                &principal.username,
                &body.store,
                &body.resource_id,
                body.slot,
                &body.key,
                "secret.delete",
                None,
                "success",
            );
            json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminSecretDeleteResponse { existed },
            )
        }
        Err(e) => {
            write_admin_secret_audit(
                &principal.username,
                &body.store,
                &body.resource_id,
                body.slot,
                &body.key,
                "secret.delete",
                None,
                "failure",
            );
            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
        }
    }
}

pub(crate) fn handle_admin_secret_write(
    state: &AdminRuntimeState,
    operator: &str,
    body: &underwrap_protocol::service::AdminSecretWriteRequest,
) -> Response<Full<Bytes>> {
    let secret_id = match validate_and_compose_secret_id(&body.resource_id, body.slot, &body.key) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let material = match base64::engine::general_purpose::STANDARD.decode(&body.material_b64) {
        Ok(material) => material,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("invalid base64 payload: {e}"),
            );
        }
    };

    let store = match admin_store(state, &body.store) {
        Ok(store) => store,
        Err(resp) => return resp,
    };

    // Admin writes are always allowed. Per-resource write permissions
    // on the policy document gate wrapper-side writes only — admin
    // sessions can't be locked out of their own backend.

    match write_resource_entry(store.as_ref(), &secret_id, &material) {
        Ok(()) => {
            write_admin_secret_audit(
                operator,
                &body.store,
                &body.resource_id,
                body.slot,
                &body.key,
                "secret.write",
                None,
                "success",
            );
            json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminOkResponse { ok: true },
            )
        }
        Err(e) => {
            write_admin_secret_audit(
                operator,
                &body.store,
                &body.resource_id,
                body.slot,
                &body.key,
                "secret.write",
                None,
                "failure",
            );
            error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string())
        }
    }
}

/// Atomically rename a per-slot entry inside a resource blob.
/// One load-modify-save of the resource blob: the previous key is
/// removed and the new key is inserted in the same in-memory pass,
/// so disk never holds both entries (or neither) — `atomic_write`
/// in the storage layer guarantees the file flip is a single rename.
/// Optionally overwrites material in the same step.
pub(crate) fn handle_admin_secret_rename(
    state: &AdminRuntimeState,
    operator: &str,
    body: &underwrap_protocol::service::AdminSecretRenameRequest,
) -> Response<Full<Bytes>> {
    let audit_failure = || {
        write_admin_secret_rename_audit(
            operator,
            &body.store,
            &body.resource_id,
            body.slot,
            &body.from_key,
            &body.to_key,
            "failure",
        );
    };
    let from_id = match validate_and_compose_secret_id(&body.resource_id, body.slot, &body.from_key)
    {
        Ok(id) => id,
        Err(resp) => {
            audit_failure();
            return resp;
        }
    };
    let to_id = match validate_and_compose_secret_id(&body.resource_id, body.slot, &body.to_key) {
        Ok(id) => id,
        Err(resp) => {
            audit_failure();
            return resp;
        }
    };
    let store = match admin_store(state, &body.store) {
        Ok(store) => store,
        Err(resp) => {
            audit_failure();
            return resp;
        }
    };
    let blob = match load_resource_blob(store.as_ref(), &body.resource_id) {
        Ok(b) => b,
        Err(e) => {
            audit_failure();
            return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
        }
    };
    let mut entries = match expand_blob_to_entries(&body.resource_id, &blob) {
        Ok(m) => m,
        Err(e) => {
            audit_failure();
            return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
        }
    };

    // The from-entry must exist; the to-entry must not (unless this
    // is a same-key rename, which collapses to a pure material write).
    let same_key = from_id == to_id;
    let Some(existing) = entries.get(&from_id).cloned() else {
        audit_failure();
        return error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("no entry under {from_id}"),
        );
    };
    if !same_key && entries.contains_key(&to_id) {
        audit_failure();
        return error_response(
            StatusCode::CONFLICT,
            "target_key_exists",
            &format!("target key {to_id} already exists; delete it first"),
        );
    }

    // Resolve material: caller-provided overrides; else carry the
    // existing value across.
    let new_material: SecretBytes = match &body.material_b64 {
        None => existing,
        Some(b64) => match base64::engine::general_purpose::STANDARD.decode(b64) {
            Ok(bytes) => SecretBytes::from(bytes),
            Err(e) => {
                audit_failure();
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    &format!("invalid base64 payload: {e}"),
                );
            }
        },
    };

    if !same_key {
        entries.remove(&from_id);
    }
    entries.insert(to_id.clone(), new_material);

    let new_blob = match blob_from_resource_entries(&body.resource_id, &entries) {
        Ok(b) => b,
        Err(e) => {
            audit_failure();
            return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
        }
    };
    if let Err(e) = save_resource_blob(store.as_ref(), &body.resource_id, &new_blob) {
        audit_failure();
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        );
    }
    write_admin_secret_rename_audit(
        operator,
        &body.store,
        &body.resource_id,
        body.slot,
        &body.from_key,
        &body.to_key,
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminOkResponse { ok: true },
    )
}

pub(crate) fn router() -> Router<Arc<AdminRuntimeState>> {
    Router::new()
        .route("/v1/admin/secret/read", post(axum_read))
        .route("/v1/admin/secret/write", post(axum_write))
        .route("/v1/admin/secret/list", post(axum_list))
        .route("/v1/admin/secret/delete", post(axum_delete))
        .route("/v1/admin/secret/rename", post(axum_rename))
}

async fn axum_read(
    State(state): State<Arc<AdminRuntimeState>>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminSecretReadRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_secret_read(&state, &parsed)
}

async fn axum_write(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminSecretWriteRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_secret_write(&state, &principal.username, &parsed)
}

async fn axum_list(
    State(state): State<Arc<AdminRuntimeState>>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminSecretListRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_secret_list(&state, &parsed)
}

async fn axum_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminSecretDeleteRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_secret_delete(&state, &principal, &parsed)
}

async fn axum_rename(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminSecretRenameRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_secret_rename(&state, &principal.username, &parsed)
}

#[cfg(test)]
mod tests;
