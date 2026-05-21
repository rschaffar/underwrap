//! Per-key admin secret CRUD: `/v1/admin/secret/{read, write, list,
//! delete, rename}`.
//!
//! Translates structured `(resource_id, slot, key)` tuples through the
//! resource-blob layout for per-key reads/writes; the `rename` handler
//! is the only multi-step one and is intentionally a single
//! load-modify-save against the blob so the file flip stays atomic.

use base64::Engine;
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use tracing::info;

use underwrap_protocol::message::SecretBytes;
use underwrap_store::{
    blob_from_resource_entries, expand_blob_to_entries, load_resource_blob, save_resource_blob,
};

use crate::http::responses::{error_response, json_response, parse_json_body};
use crate::runtime_state::AdminRuntimeState;
use crate::storage::secret_entries::{
    delete_resource_entry, list_secret_entries, read_resource_entry,
    validate_and_compose_secret_id, write_resource_entry,
};

pub(crate) async fn handle_admin_secret_read(
    state: &AdminRuntimeState,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    use crate::server::admin_store;
    let body = match parse_json_body::<underwrap_protocol::service::AdminSecretReadRequest>(request)
        .await
    {
        Ok(b) => b,
        Err(resp) => return resp,
    };
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

pub(crate) async fn handle_admin_secret_write_endpoint(
    state: &AdminRuntimeState,
    actor: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    use crate::server::admin_store;
    let body = match parse_json_body::<underwrap_protocol::service::AdminSecretWriteRequest>(
        request,
    )
    .await
    {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    match admin_store(state, &body.store) {
        Ok(_) => handle_admin_secret_write(state, actor, body),
        Err(resp) => resp,
    }
}

pub(crate) async fn handle_admin_secret_list(
    state: &AdminRuntimeState,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    use crate::server::admin_store;
    let body = match parse_json_body::<underwrap_protocol::service::AdminSecretListRequest>(request)
        .await
    {
        Ok(b) => b,
        Err(resp) => return resp,
    };
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

pub(crate) async fn handle_admin_secret_delete(
    state: &AdminRuntimeState,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    // Admin deletes are always allowed once per-store access
    // has been checked; per-resource write flags only gate
    // wrapper-side writes.
    use crate::server::admin_store;
    let body =
        match parse_json_body::<underwrap_protocol::service::AdminSecretDeleteRequest>(request)
            .await
        {
            Ok(b) => b,
            Err(resp) => return resp,
        };
    let secret_id = match validate_and_compose_secret_id(&body.resource_id, body.slot, &body.key) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    match admin_store(state, &body.store) {
        Ok(store) => match delete_resource_entry(store.as_ref(), &secret_id) {
            Ok(existed) => json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminSecretDeleteResponse { existed },
            ),
            Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
        },
        Err(resp) => resp,
    }
}

pub(crate) async fn handle_admin_secret_rename_endpoint(
    state: &AdminRuntimeState,
    actor: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    use crate::server::admin_store;
    let body =
        match parse_json_body::<underwrap_protocol::service::AdminSecretRenameRequest>(request)
            .await
        {
            Ok(b) => b,
            Err(resp) => return resp,
        };
    match admin_store(state, &body.store) {
        Ok(_) => handle_admin_secret_rename(state, actor, body),
        Err(resp) => resp,
    }
}

pub(crate) fn handle_admin_secret_write(
    state: &AdminRuntimeState,
    operator: &str,
    body: underwrap_protocol::service::AdminSecretWriteRequest,
) -> Response<Full<Bytes>> {
    use crate::server::admin_store;
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
            info!(
                audit = "secret_stored",
                operator = %operator,
                store = %body.store,
                secret = %secret_id,
            );
            json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminOkResponse { ok: true },
            )
        }
        Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
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
    body: underwrap_protocol::service::AdminSecretRenameRequest,
) -> Response<Full<Bytes>> {
    use crate::server::admin_store;
    let from_id = match validate_and_compose_secret_id(&body.resource_id, body.slot, &body.from_key)
    {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let to_id = match validate_and_compose_secret_id(&body.resource_id, body.slot, &body.to_key) {
        Ok(id) => id,
        Err(resp) => return resp,
    };
    let store = match admin_store(state, &body.store) {
        Ok(store) => store,
        Err(resp) => return resp,
    };
    let blob = match load_resource_blob(store.as_ref(), &body.resource_id) {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };
    let mut entries = match expand_blob_to_entries(&body.resource_id, &blob) {
        Ok(m) => m,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };

    // The from-entry must exist; the to-entry must not (unless this
    // is a same-key rename, which collapses to a pure material write).
    let same_key = from_id == to_id;
    let Some(existing) = entries.get(&from_id).cloned() else {
        return error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            &format!("no entry under {from_id}"),
        );
    };
    if !same_key && entries.contains_key(&to_id) {
        return error_response(
            StatusCode::CONFLICT,
            "target_key_exists",
            &format!("target key {to_id} already exists; delete it first"),
        );
    }

    // Resolve material: caller-provided overrides; else carry the
    // existing value across.
    let new_material: SecretBytes = match body.material_b64 {
        None => existing,
        Some(b64) => match base64::engine::general_purpose::STANDARD.decode(&b64) {
            Ok(bytes) => SecretBytes::from(bytes),
            Err(e) => {
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
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    };
    if let Err(e) = save_resource_blob(store.as_ref(), &body.resource_id, &new_blob) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &e.to_string(),
        );
    }
    info!(
        audit = "secret_renamed",
        operator = %operator,
        store = %body.store,
        from = %from_id,
        to = %to_id,
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminOkResponse { ok: true },
    )
}

pub(crate) async fn route(
    method: &Method,
    path: &str,
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    match (method, path) {
        (&Method::POST, "/v1/admin/secret/read") => handle_admin_secret_read(state, request).await,
        (&Method::POST, "/v1/admin/secret/write") => {
            handle_admin_secret_write_endpoint(state, &principal.username, request).await
        }
        (&Method::POST, "/v1/admin/secret/list") => handle_admin_secret_list(state, request).await,
        (&Method::POST, "/v1/admin/secret/delete") => {
            handle_admin_secret_delete(state, request).await
        }
        (&Method::POST, "/v1/admin/secret/rename") => {
            handle_admin_secret_rename_endpoint(state, &principal.username, request).await
        }
        _ => error_response(StatusCode::NOT_FOUND, "not_found", "unknown admin endpoint"),
    }
}
