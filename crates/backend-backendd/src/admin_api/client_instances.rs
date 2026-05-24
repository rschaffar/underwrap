//! Client-instance admin CRUD: `/v1/admin/client-instances/{list,
//! create, update, delete}`.
//!
//! Client instances are the mTLS identities under which frontend
//! daemons authenticate to the backend's runtime API. Each instance
//! maps to one store; a remap drops the prior-store's wrapper
//! permissions so authorizations don't silently survive across the
//! reassignment.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use underwrap_protocol::service::{
    AdminClientInstanceActionState, AdminClientInstanceCreateRequest, AdminClientInstanceListItem,
    AdminClientInstanceStatus, AdminClientInstanceUpdateRequest, AdminClientInstancesListResponse,
    AdminOkResponse, ServiceErrorResponse,
};

use crate::admin_api::AdminError;
use crate::admin_api::audit::{AuditEntity, log_admin_event};
use crate::admin_api::policy::drop_client_instance_permissions;
use crate::http::responses::Body;
use crate::runtime_state::AdminRuntimeState;
use crate::users::Principal;

fn client_instance_status_to_wire(
    status: crate::runtime_clients::ClientInstanceStatus,
) -> AdminClientInstanceStatus {
    match status {
        crate::runtime_clients::ClientInstanceStatus::Active => AdminClientInstanceStatus::Active,
        crate::runtime_clients::ClientInstanceStatus::Disabled => {
            AdminClientInstanceStatus::Disabled
        }
    }
}

fn client_instance_status_from_wire(
    status: AdminClientInstanceStatus,
) -> crate::runtime_clients::ClientInstanceStatus {
    match status {
        AdminClientInstanceStatus::Active => crate::runtime_clients::ClientInstanceStatus::Active,
        AdminClientInstanceStatus::Disabled => {
            crate::runtime_clients::ClientInstanceStatus::Disabled
        }
    }
}

fn admin_client_instance_actions(
    record: &crate::runtime_clients::ClientInstanceRecord,
) -> AdminClientInstanceActionState {
    AdminClientInstanceActionState {
        can_disable: record.status == crate::runtime_clients::ClientInstanceStatus::Active,
        can_enable: record.status == crate::runtime_clients::ClientInstanceStatus::Disabled,
    }
}

fn current_store_names(state: &AdminRuntimeState) -> HashMap<String, String> {
    state
        .stores
        .read()
        .unwrap()
        .records
        .iter()
        .map(|(id, loaded)| (id.clone(), loaded.record.name.clone()))
        .collect()
}

fn admin_client_instance_list_item(
    record: &crate::runtime_clients::ClientInstanceRecord,
    current_store_name: String,
    actions: AdminClientInstanceActionState,
) -> AdminClientInstanceListItem {
    AdminClientInstanceListItem {
        id: record.id.clone(),
        display_name: record.display_name.clone(),
        current_store_id: record.current_store_id.clone(),
        current_store_name,
        status: client_instance_status_to_wire(record.status),
        last_seen_at: record.last_seen_at,
        current_credential_fingerprint: record.current_credential.cert_fingerprint.clone(),
        auto_create_wrapper_assets: record.auto_create_wrapper_assets,
        actions,
    }
}

/// Look up the display name of `store_id` in the live registry.
fn resolve_store_name(state: &AdminRuntimeState, store_id: &str) -> Result<String, AdminError> {
    state
        .stores
        .read()
        .unwrap()
        .records
        .get(store_id)
        .map(|loaded| loaded.record.name.clone())
        .ok_or_else(|| {
            AdminError::internal_with_code(
                "unknown_store",
                "client instance references an unknown store",
            )
        })
}

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(
            handle_admin_client_instances_list,
            handle_admin_client_instance_create
        ))
        .routes(routes!(
            handle_admin_client_instance_update,
            handle_admin_client_instance_delete
        ))
}

#[utoipa::path(
    get,
    path = "/v1/admin/client-instances",
    tag = "client_instances",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "All client instances with action state and store mapping.", body = AdminClientInstancesListResponse),
    ),
)]
pub(crate) async fn handle_admin_client_instances_list(
    State(state): State<Arc<AdminRuntimeState>>,
) -> Result<Json<AdminClientInstancesListResponse>, AdminError> {
    let store_names = current_store_names(&state);
    let mut items = Vec::new();
    for record in state.client_instances.read().unwrap().values() {
        let Some(current_store_name) = store_names.get(&record.current_store_id).cloned() else {
            return Err(AdminError::internal_with_code(
                "unknown_store",
                "client instance references an unknown store",
            ));
        };
        let actions = admin_client_instance_actions(record);
        items.push(admin_client_instance_list_item(
            record,
            current_store_name,
            actions,
        ));
    }
    items.sort_by(|a, b| a.display_name.cmp(&b.display_name).then(a.id.cmp(&b.id)));
    Ok(Json(AdminClientInstancesListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/client-instances",
    tag = "client_instances",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminClientInstanceCreateRequest,
    responses(
        (status = 200, description = "Client instance registered.", body = AdminClientInstanceListItem),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 409, description = "Instance id already exists.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_client_instance_create(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminClientInstanceCreateRequest>,
) -> Result<Json<AdminClientInstanceListItem>, AdminError> {
    let display_name = body.display_name.trim();
    if display_name.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_display_name",
            "display name must not be empty",
        ));
    }
    let display_name = display_name.to_string();
    let current_credential = crate::runtime_clients::current_credential_from_pem(
        body.cert_pem.clone(),
    )
    .map_err(|e| {
        AdminError::bad_request(
            "invalid_client_certificate",
            format!("parsing client certificate: {e:#}"),
        )
    })?;
    let cert_fingerprint = current_credential.cert_fingerprint.clone();
    {
        let stores = state.stores.read().unwrap();
        if !stores.records.contains_key(&body.current_store_id) {
            return Err(AdminError::bad_request("unknown_store", "unknown store"));
        }
    }
    let id = crate::ids::fresh_id("inst", |candidate| {
        state
            .client_instances
            .read()
            .unwrap()
            .contains_key(candidate)
    });
    let mut instances = state.client_instances.write().unwrap();
    if instances
        .values()
        .any(|existing| existing.display_name.eq_ignore_ascii_case(&display_name))
    {
        return Err(AdminError::conflict(
            "display_name_already_exists",
            format!("client instance display name '{display_name}' already exists"),
        ));
    }
    if instances
        .values()
        .any(|existing| existing.current_credential.cert_fingerprint == cert_fingerprint)
    {
        return Err(AdminError::conflict(
            "credential_already_exists",
            "client credential fingerprint already exists",
        ));
    }
    let record = crate::runtime_clients::ClientInstanceRecord {
        id: id.clone(),
        display_name,
        current_store_id: body.current_store_id.clone(),
        status: client_instance_status_from_wire(body.status),
        last_seen_at: None,
        current_credential,
        auto_create_wrapper_assets: body.auto_create_wrapper_assets.unwrap_or(false),
    };
    instances.insert(id.clone(), record.clone());
    if let Err(e) = crate::runtime_clients::save_client_instance(&state.clients_dir, &record) {
        instances.remove(&id);
        return Err(AdminError::internal_with_code(
            "write_client_instances_failed",
            format!("{e:#}"),
        ));
    }
    drop(instances);
    log_admin_event(
        "client-instance.create",
        &principal,
        AuditEntity::ClientInstance { id: &id },
        None,
        "success",
    );
    let current_store_name = resolve_store_name(&state, &record.current_store_id)?;
    let actions = admin_client_instance_actions(&record);
    Ok(Json(admin_client_instance_list_item(
        &record,
        current_store_name,
        actions,
    )))
}

#[utoipa::path(
    put,
    path = "/v1/admin/client-instances/{id}",
    tag = "client_instances",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Client instance id.")),
    request_body = AdminClientInstanceUpdateRequest,
    responses(
        (status = 200, description = "Instance updated. Remapping to a new store drops the prior store's wrapper permissions for this instance.", body = AdminClientInstanceListItem),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Instance / target store not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_client_instance_update(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
    Body(body): Body<AdminClientInstanceUpdateRequest>,
) -> Result<Json<AdminClientInstanceListItem>, AdminError> {
    let mut instances = state.client_instances.write().unwrap();
    let existing = instances
        .get(&id)
        .cloned()
        .ok_or_else(AdminError::client_instance_not_found)?;
    let old_store_id = existing.current_store_id.clone();
    let mut updated = existing;
    if let Some(display_name) = &body.display_name {
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err(AdminError::bad_request(
                "invalid_display_name",
                "display name must not be empty",
            ));
        }
        let display_name = display_name.to_string();
        if instances.iter().any(|(other_id, other)| {
            other_id != &id && other.display_name.eq_ignore_ascii_case(&display_name)
        }) {
            return Err(AdminError::conflict(
                "display_name_already_exists",
                format!("client instance display name '{display_name}' already exists"),
            ));
        }
        updated.display_name = display_name;
    }
    if let Some(current_store_id) = &body.current_store_id {
        if !state
            .stores
            .read()
            .unwrap()
            .records
            .contains_key(current_store_id)
        {
            return Err(AdminError::bad_request("unknown_store", "unknown store"));
        }
        updated.current_store_id = current_store_id.clone();
    }
    if let Some(status) = body.status {
        updated.status = client_instance_status_from_wire(status);
    }
    if let Some(cert_pem) = &body.cert_pem {
        let new_credential = crate::runtime_clients::current_credential_from_pem(cert_pem.clone())
            .map_err(|e| {
                AdminError::bad_request(
                    "invalid_client_certificate",
                    format!("parsing client certificate: {e:#}"),
                )
            })?;
        if instances.iter().any(|(other_id, other)| {
            other_id != &id
                && other.current_credential.cert_fingerprint == new_credential.cert_fingerprint
        }) {
            return Err(AdminError::conflict(
                "credential_already_exists",
                "client credential fingerprint already exists",
            ));
        }
        updated.current_credential = new_credential;
    }
    if let Some(flag) = body.auto_create_wrapper_assets {
        updated.auto_create_wrapper_assets = flag;
    }
    instances.insert(id.clone(), updated.clone());
    if let Err(e) = crate::runtime_clients::save_client_instance(&state.clients_dir, &updated) {
        return Err(AdminError::internal_with_code(
            "write_client_instances_failed",
            format!("{e:#}"),
        ));
    }
    drop(instances);
    if old_store_id != updated.current_store_id
        && let Err(e) = drop_client_instance_permissions(&state, &principal, &old_store_id, &id)
    {
        // Best-effort: log and continue. The remap already persisted
        // to disk, so failing the response with 500 would leave the
        // caller thinking the update did not take while the registry
        // already holds the new mapping. Mirrors the delete path's
        // treatment of the same cascade.
        tracing::warn!(
            client_instance = %id,
            store = %old_store_id,
            err = %format!("{e:#}"),
            "failed to strip wrapper permissions for remapped client instance",
        );
    }
    log_admin_event(
        "client-instance.update",
        &principal,
        AuditEntity::ClientInstance { id: &id },
        None,
        "success",
    );
    let current_store_name = resolve_store_name(&state, &updated.current_store_id)?;
    let actions = admin_client_instance_actions(&updated);
    Ok(Json(admin_client_instance_list_item(
        &updated,
        current_store_name,
        actions,
    )))
}

#[utoipa::path(
    delete,
    path = "/v1/admin/client-instances/{id}",
    tag = "client_instances",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Client instance id.")),
    responses(
        (status = 200, description = "Instance removed; its wrapper permissions are dropped from the mapped store.", body = AdminOkResponse),
        (status = 404, description = "Instance not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_client_instance_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let mut instances = state.client_instances.write().unwrap();
    let removed = instances
        .remove(&id)
        .ok_or_else(AdminError::client_instance_not_found)?;
    if let Err(e) = crate::runtime_clients::delete_client_instance(&state.clients_dir, &id) {
        // Roll the in-memory state back so the next request sees the same
        // view as the on-disk file.
        instances.insert(id.clone(), removed);
        return Err(AdminError::internal_with_code(
            "write_client_instances_failed",
            format!("{e:#}"),
        ));
    }
    drop(instances);
    // Cascade: strip every wrapper permission keyed on this client
    // instance id from the policy document of the store the instance
    // was last bound to. Without this, the rows linger forever as
    // "stale" entries that the SPA renders as nameless columns in the
    // wrapper-permissions table and that grant the same authorization
    // back if the same client instance id is ever reused.
    if let Err(e) =
        drop_client_instance_permissions(&state, &principal, &removed.current_store_id, &id)
    {
        // Best-effort: log and continue. The instance is already gone from
        // the registry, so leaving orphaned policy rows is preferable to
        // failing the delete (and leaving the operator with no way to
        // remove the client instance at all).
        tracing::warn!(
            client_instance = %id,
            store = %removed.current_store_id,
            err = %format!("{e:#}"),
            "failed to strip wrapper permissions for deleted client instance",
        );
    }
    log_admin_event(
        "client-instance.delete",
        &principal,
        AuditEntity::ClientInstance { id: &id },
        None,
        "success",
    );
    Ok(Json(AdminOkResponse { ok: true }))
}

#[cfg(test)]
mod tests;
