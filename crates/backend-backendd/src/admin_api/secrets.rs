//! Per-key admin secret CRUD: `/v1/admin/secret/{read, write, list,
//! delete, rename}`.
//!
//! Translates structured `(resource_id, slot, key)` tuples through the
//! resource-blob layout for per-key reads/writes; the `rename` handler
//! is the only multi-step one and is intentionally a single
//! load-modify-save against the blob so the file flip stays atomic.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use base64::Engine;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource::ResourceSlot;
use underwrap_protocol::service::{
    AdminOkResponse, AdminSecretDeleteRequest, AdminSecretDeleteResponse, AdminSecretListRequest,
    AdminSecretListResponse, AdminSecretReadRequest, AdminSecretReadResponse,
    AdminSecretRenameRequest, AdminSecretWriteRequest, ServiceErrorResponse,
};
use underwrap_store::{
    blob_from_resource_entries, expand_blob_to_entries, load_resource_blob, save_resource_blob,
};

use crate::admin_api::AdminError;
use crate::admin_api::audit::log_admin_event;
use crate::admin_api::stores::admin_store;
use crate::http::responses::Body;
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

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_secret_read))
        .routes(routes!(handle_admin_secret_write))
        .routes(routes!(handle_admin_secret_list))
        .routes(routes!(handle_admin_secret_delete))
        .routes(routes!(handle_admin_secret_rename))
}

#[utoipa::path(
    post,
    path = "/v1/admin/secret/read",
    tag = "secrets",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminSecretReadRequest,
    responses(
        (status = 200, description = "Secret value for the (`resource_id`, `slot`, `key`) tuple, base64-encoded.", body = AdminSecretReadResponse),
        (status = 404, description = "Store / resource / key not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_secret_read(
    State(state): State<Arc<AdminRuntimeState>>,
    Body(body): Body<AdminSecretReadRequest>,
) -> Result<Json<AdminSecretReadResponse>, AdminError> {
    let secret_id = validate_and_compose_secret_id(&body.resource_id, body.slot, &body.key)?;
    let store = admin_store(&state, &body.store)?;
    let material =
        read_resource_entry(store.as_ref(), &secret_id).map_err(AdminError::admin)?;
    Ok(Json(AdminSecretReadResponse {
        material_b64: material.map(|material| {
            let bytes: &[u8] = material.as_ref();
            base64::engine::general_purpose::STANDARD.encode(bytes)
        }),
    }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/secret/list",
    tag = "secrets",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminSecretListRequest,
    responses(
        (status = 200, description = "Keys present in the requested resource slot.", body = AdminSecretListResponse),
        (status = 404, description = "Store / resource not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_secret_list(
    State(state): State<Arc<AdminRuntimeState>>,
    Body(body): Body<AdminSecretListRequest>,
) -> Result<Json<AdminSecretListResponse>, AdminError> {
    let store = admin_store(&state, &body.store)?;
    let entries = list_secret_entries(store.as_ref(), body.resource_id.as_deref())
        .map_err(AdminError::admin)?;
    Ok(Json(AdminSecretListResponse { entries }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/secret/delete",
    tag = "secrets",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminSecretDeleteRequest,
    responses(
        (status = 200, description = "Secret removed.", body = AdminSecretDeleteResponse),
        (status = 404, description = "Store / resource / key not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_secret_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminSecretDeleteRequest>,
) -> Result<Json<AdminSecretDeleteResponse>, AdminError> {
    // Admin deletes are always allowed once per-store access
    // has been checked; per-resource write flags only gate
    // wrapper-side writes.
    let secret_id = validate_and_compose_secret_id(&body.resource_id, body.slot, &body.key)?;
    let store = admin_store(&state, &body.store)?;
    let outcome = delete_resource_entry(store.as_ref(), &secret_id);
    let result = if outcome.is_ok() {
        "success"
    } else {
        "failure"
    };
    write_admin_secret_audit(
        &principal.username,
        &body.store,
        &body.resource_id,
        body.slot,
        &body.key,
        "secret.delete",
        None,
        result,
    );
    let existed = outcome.map_err(AdminError::admin)?;
    Ok(Json(AdminSecretDeleteResponse { existed }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/secret/write",
    tag = "secrets",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminSecretWriteRequest,
    responses(
        (status = 200, description = "Secret written.", body = AdminOkResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Store / resource not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_secret_write(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminSecretWriteRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let secret_id = validate_and_compose_secret_id(&body.resource_id, body.slot, &body.key)?;
    let material = base64::engine::general_purpose::STANDARD
        .decode(&body.material_b64)
        .map_err(|e| {
            AdminError::bad_request("invalid_request", format!("invalid base64 payload: {e}"))
        })?;
    let store = admin_store(&state, &body.store)?;

    // Admin writes are always allowed. Per-resource write permissions
    // on the policy document gate wrapper-side writes only — admin
    // sessions can't be locked out of their own backend.
    let outcome = write_resource_entry(store.as_ref(), &secret_id, &material);
    let result = if outcome.is_ok() {
        "success"
    } else {
        "failure"
    };
    write_admin_secret_audit(
        &principal.username,
        &body.store,
        &body.resource_id,
        body.slot,
        &body.key,
        "secret.write",
        None,
        result,
    );
    outcome.map_err(AdminError::admin)?;
    Ok(Json(AdminOkResponse { ok: true }))
}

/// Atomically rename a per-slot entry inside a resource blob.
/// One load-modify-save of the resource blob: the previous key is
/// removed and the new key is inserted in the same in-memory pass,
/// so disk never holds both entries (or neither) — `atomic_write`
/// in the storage layer guarantees the file flip is a single rename.
/// Optionally overwrites material in the same step.
#[utoipa::path(
    post,
    path = "/v1/admin/secret/rename",
    tag = "secrets",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminSecretRenameRequest,
    responses(
        (status = 200, description = "Secret renamed.", body = AdminOkResponse),
        (status = 404, description = "Store / resource / key not found.", body = ServiceErrorResponse),
        (status = 409, description = "Target key already exists.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_secret_rename(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminSecretRenameRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let operator = &principal.username;
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
    let result = rename_inner(&state, &body).await;
    match &result {
        Ok(_) => write_admin_secret_rename_audit(
            operator,
            &body.store,
            &body.resource_id,
            body.slot,
            &body.from_key,
            &body.to_key,
            "success",
        ),
        Err(_) => audit_failure(),
    }
    result.map(Json)
}

async fn rename_inner(
    state: &AdminRuntimeState,
    body: &AdminSecretRenameRequest,
) -> Result<AdminOkResponse, AdminError> {
    let from_id = validate_and_compose_secret_id(&body.resource_id, body.slot, &body.from_key)?;
    let to_id = validate_and_compose_secret_id(&body.resource_id, body.slot, &body.to_key)?;
    let store = admin_store(state, &body.store)?;
    let blob =
        load_resource_blob(store.as_ref(), &body.resource_id).map_err(AdminError::admin)?;
    let mut entries =
        expand_blob_to_entries(&body.resource_id, &blob).map_err(AdminError::admin)?;

    // The from-entry must exist; the to-entry must not (unless this
    // is a same-key rename, which collapses to a pure material write).
    let same_key = from_id == to_id;
    let Some(existing) = entries.get(&from_id).cloned() else {
        return Err(AdminError::not_found(format!("no entry under {from_id}")));
    };
    if !same_key && entries.contains_key(&to_id) {
        return Err(AdminError::conflict(
            "target_key_exists",
            format!("target key {to_id} already exists; delete it first"),
        ));
    }

    // Resolve material: caller-provided overrides; else carry the
    // existing value across.
    let new_material: SecretBytes = match &body.material_b64 {
        None => existing,
        Some(b64) => SecretBytes::from(
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| {
                    AdminError::bad_request(
                        "invalid_request",
                        format!("invalid base64 payload: {e}"),
                    )
                })?,
        ),
    };

    if !same_key {
        entries.remove(&from_id);
    }
    entries.insert(to_id.clone(), new_material);

    let new_blob =
        blob_from_resource_entries(&body.resource_id, &entries).map_err(AdminError::admin)?;
    save_resource_blob(store.as_ref(), &body.resource_id, &new_blob)
        .map_err(|e| AdminError::internal("saving resource blob", e))?;
    Ok(AdminOkResponse { ok: true })
}

#[cfg(test)]
mod tests;
