//! Per-key admin secret CRUD under
//! `/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}`.
//!
//! Handles only the genuinely flat-KV slots: `env` (name → value) and
//! `files` (path → bytes). Libsecret content is structured and goes
//! through [`super::libsecret`]; requests targeting the `libsecret`
//! slot here are rejected with `slot_not_supported` and a pointer at
//! the structured endpoints.
//!
//! Translates the URL-addressed `(store, resource_id, slot, key)`
//! tuple through the resource-blob layout for per-key reads/writes;
//! the `rename` handler is the only multi-step one and is
//! intentionally a single load-modify-save against the blob so the
//! file flip stays atomic.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use underwrap_backend_store::{load_resource_blob, save_resource_blob};
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource::ResourceSlot;
use underwrap_protocol::service::{
    AdminOkResponse, AdminSecretDeleteResponse, AdminSecretListResponse, AdminSecretReadResponse,
    AdminSecretRenameRequest, AdminSecretWriteRequest, ServiceErrorResponse,
};

use crate::admin_api::AdminError;
use crate::admin_api::audit::{AuditEntity, log_admin_event, result_label};
use crate::admin_api::csrf::Principal;
use crate::admin_api::stores::admin_store;
use crate::admin_api::util::{IntoInternal, decode_b64, encode_b64};
use crate::http::responses::Body;
use crate::runtime_state::ListenerState;
use crate::storage::secret_entries::{
    delete_resource_entry, flat_slot_entries_mut, list_secret_entries, read_resource_entry,
    validate_and_compose_secret_id, write_resource_entry,
};

/// Parse a slot segment from the URL path or surface a 400 error.
///
/// The flat-KV admin endpoints in this module address only the `env`
/// and `files` slots. Libsecret content is structured (Service →
/// Collections → Items + an alias table) and lives behind the
/// `/v1/admin/stores/{sid}/resources/{rid}/libsecret/*` endpoints in
/// [`super::libsecret`]; rejecting it here keeps the two surfaces
/// from drifting and gives operators a clear pointer when they hit
/// a stale URL.
fn parse_slot(slot: &str) -> Result<ResourceSlot, AdminError> {
    match ResourceSlot::parse(slot) {
        Some(s @ (ResourceSlot::Env | ResourceSlot::Files)) => Ok(s),
        Some(ResourceSlot::Libsecret) => Err(AdminError::bad_request(
            "slot_not_supported",
            "libsecret content is addressed through the structured \
             /v1/admin/stores/{sid}/resources/{rid}/libsecret/* endpoints, \
             not the flat-KV secrets endpoints",
        )),
        None => Err(AdminError::bad_request(
            "invalid_request",
            format!("unknown slot '{slot}'; expected one of env, files"),
        )),
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct SecretListQuery {
    /// Restrict listing to a single resource. Omit to return entries
    /// for every resource in the store.
    pub(crate) resource_id: Option<String>,
}

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<ListenerState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_secrets_list))
        .routes(routes!(
            handle_admin_secret_read,
            handle_admin_secret_write,
            handle_admin_secret_delete
        ))
        .routes(routes!(handle_admin_secret_rename))
}

#[utoipa::path(
    get,
    path = "/v1/admin/stores/{id}/secrets",
    tag = "secrets",
    security(("session_cookie" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("resource_id" = Option<String>, Query, description = "Restrict listing to a single resource."),
    ),
    responses(
        (status = 200, description = "Per-slot entries in the store (optionally filtered by resource id).", body = AdminSecretListResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_secrets_list(
    State(state): State<Arc<ListenerState>>,
    Path(store_id): Path<String>,
    Query(query): Query<SecretListQuery>,
) -> Result<Json<AdminSecretListResponse>, AdminError> {
    let store = admin_store(&state, &store_id)?;
    let entries = list_secret_entries(store.as_ref(), query.resource_id.as_deref())
        .into_internal("listing secret entries")?;
    Ok(Json(AdminSecretListResponse { entries }))
}

#[utoipa::path(
    get,
    path = "/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}",
    tag = "secrets",
    security(("session_cookie" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("slot" = String, Path, description = "Slot name: env, files, libsecret."),
        ("key" = String, Path, description = "Per-slot entry key."),
    ),
    responses(
        (status = 200, description = "Secret value, base64-encoded.", body = AdminSecretReadResponse),
        (status = 404, description = "Store / resource / key not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_secret_read(
    State(state): State<Arc<ListenerState>>,
    Path((store_id, resource_id, slot, key)): Path<(String, String, String, String)>,
) -> Result<Json<AdminSecretReadResponse>, AdminError> {
    let slot = parse_slot(&slot)?;
    let secret_id = validate_and_compose_secret_id(&resource_id, slot, &key)?;
    let store = admin_store(&state, &store_id)?;
    let material =
        read_resource_entry(store.as_ref(), &secret_id).into_internal("reading resource entry")?;
    Ok(Json(AdminSecretReadResponse {
        material_b64: material.map(|material| encode_b64(material.as_ref())),
    }))
}

#[utoipa::path(
    put,
    path = "/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}",
    tag = "secrets",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("slot" = String, Path, description = "Slot name: env, files, libsecret."),
        ("key" = String, Path, description = "Per-slot entry key."),
    ),
    request_body = AdminSecretWriteRequest,
    responses(
        (status = 200, description = "Secret written.", body = AdminOkResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Store / resource not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_secret_write(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id, slot, key)): Path<(String, String, String, String)>,
    Body(body): Body<AdminSecretWriteRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let slot = parse_slot(&slot)?;
    let secret_id = validate_and_compose_secret_id(&resource_id, slot, &key)?;
    let material = decode_b64(&body.material_b64)?;
    let store = admin_store(&state, &store_id)?;

    // Admin writes are always allowed. Per-resource write permissions
    // on the policy document gate wrapper-side writes only — admin
    // sessions can't be locked out of their own backend.
    let outcome = write_resource_entry(store.as_ref(), &secret_id, &material);
    log_admin_event(
        "secret.write",
        &principal,
        AuditEntity::Secret {
            store: &store_id,
            resource_id: &resource_id,
            slot,
            key: &key,
        },
        None,
        result_label(&outcome),
    );
    outcome.into_internal("writing secret entry")?;
    Ok(Json(AdminOkResponse { ok: true }))
}

#[utoipa::path(
    delete,
    path = "/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}",
    tag = "secrets",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("slot" = String, Path, description = "Slot name: env, files, libsecret."),
        ("key" = String, Path, description = "Per-slot entry key."),
    ),
    responses(
        (status = 200, description = "Secret removed.", body = AdminSecretDeleteResponse),
        (status = 404, description = "Store / resource / key not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_secret_delete(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id, slot, key)): Path<(String, String, String, String)>,
) -> Result<Json<AdminSecretDeleteResponse>, AdminError> {
    let slot = parse_slot(&slot)?;
    // Admin deletes are always allowed once per-store access
    // has been checked; per-resource write flags only gate
    // wrapper-side writes.
    let secret_id = validate_and_compose_secret_id(&resource_id, slot, &key)?;
    let store = admin_store(&state, &store_id)?;
    let outcome = delete_resource_entry(store.as_ref(), &secret_id);
    log_admin_event(
        "secret.delete",
        &principal,
        AuditEntity::Secret {
            store: &store_id,
            resource_id: &resource_id,
            slot,
            key: &key,
        },
        None,
        result_label(&outcome),
    );
    let existed = outcome.into_internal("deleting secret entry")?;
    Ok(Json(AdminSecretDeleteResponse { existed }))
}

/// Rename a per-slot entry inside a resource blob.
/// One load-modify-save removes the source key and inserts the destination in
/// the same in-memory pass; the storage layer then replaces the resource file
/// with one atomic rename. Concurrent whole-blob writers remain
/// last-writer-wins. Optionally overwrites material in the same step.
#[utoipa::path(
    post,
    path = "/v1/admin/stores/{id}/secrets/{rid}/{slot}/{key}/rename",
    tag = "secrets",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("slot" = String, Path, description = "Slot name."),
        ("key" = String, Path, description = "Source key (`from_key`)."),
    ),
    request_body = AdminSecretRenameRequest,
    responses(
        (status = 200, description = "Secret renamed.", body = AdminOkResponse),
        (status = 404, description = "Store / resource / key not found.", body = ServiceErrorResponse),
        (status = 409, description = "Target key already exists.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_secret_rename(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id, slot, from_key)): Path<(String, String, String, String)>,
    Body(body): Body<AdminSecretRenameRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let slot = parse_slot(&slot)?;
    let result = rename_inner(&state, &store_id, &resource_id, slot, &from_key, &body).await;
    let operation = format!("rename->{}", body.to_key);
    log_admin_event(
        "secret.rename",
        &principal,
        AuditEntity::Secret {
            store: &store_id,
            resource_id: &resource_id,
            slot,
            key: &from_key,
        },
        Some(&operation),
        result_label(&result),
    );
    result.map(Json)
}

async fn rename_inner(
    state: &ListenerState,
    store_id: &str,
    resource_id: &str,
    slot: ResourceSlot,
    from_key: &str,
    body: &AdminSecretRenameRequest,
) -> Result<AdminOkResponse, AdminError> {
    let from_id = validate_and_compose_secret_id(resource_id, slot, from_key)?;
    let to_id = validate_and_compose_secret_id(resource_id, slot, &body.to_key)?;
    let store = admin_store(state, store_id)?;
    let mut blob =
        load_resource_blob(store.as_ref(), resource_id).into_internal("loading resource blob")?;
    let entries =
        flat_slot_entries_mut(&mut blob, slot).into_internal("selecting flat resource slot")?;

    // The from-entry must exist; the to-entry must not (unless this
    // is a same-key rename, which collapses to a pure material write).
    let same_key = from_id == to_id;
    let Some(existing) = entries.get(from_key).cloned() else {
        return Err(AdminError::not_found(format!("no entry under {from_id}")));
    };
    if !same_key && entries.contains_key(&body.to_key) {
        return Err(AdminError::conflict(
            "target_key_exists",
            format!("target key {to_id} already exists; delete it first"),
        ));
    }

    // Resolve material: caller-provided overrides; else carry the
    // existing value across.
    let new_material: SecretBytes = match &body.material_b64 {
        None => existing,
        Some(b64) => SecretBytes::from(decode_b64(b64)?),
    };

    if !same_key {
        entries.remove(from_key);
    }
    entries.insert(body.to_key.clone(), new_material);

    save_resource_blob(store.as_ref(), resource_id, &blob).into_internal("saving resource blob")?;
    Ok(AdminOkResponse { ok: true })
}

#[cfg(test)]
mod tests;
