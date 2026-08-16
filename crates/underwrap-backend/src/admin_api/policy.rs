//! Wrapper-permission admin CRUD under
//! `/v1/admin/stores/{id}/wrapper-permissions`.
//!
//! Plus `drop_client_instance_permissions`, which the client-instance
//! admin module calls when an instance is reassigned to a different
//! store.

use std::sync::Arc;

use anyhow::Result;
use axum::Json;
use axum::extract::{Path, State};
use underwrap_protocol::policy::WrapperPermission;
use underwrap_protocol::service::{
    AdminOkResponse, AdminWrapperPermissionUpsertRequest, AdminWrapperPermissionsListResponse,
    ServiceErrorResponse,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::audit::{AuditEntity, log_admin_event};
use crate::admin_api::csrf::Principal;
use crate::admin_api::references::{
    require_client_instance_in_store, require_pair, require_resource,
};
use crate::admin_api::stores::admin_store;
use crate::admin_api::util::IntoInternal;
use crate::http::responses::Body;
use crate::runtime_state::ListenerState;
use crate::storage::documents::{load_policy_document, save_policy_document};

/// Strip every wrapper permission whose `client_instance` matches
/// `client_instance_id` from the policy document of `store_id`. Used
/// when a client instance is reassigned to a different store, so its
/// authorizations on the previous store do not silently resurface if
/// it is ever remapped back.
///
/// Behaviour by `RegistryLookupError` arm:
/// - `Locked`: the store is locked, so we can't read or rewrite its
///   policy document. Returns `Ok(())` after emitting a structured
///   `tracing::warn!` plus a `client-instance.orphan-permissions`
///   audit event so operators can find the orphans. The next time
///   the store is unlocked, the rows can be cleaned up by hand.
/// - `NotFound`: the previous store was already deleted, so there
///   is no policy document to clean. Logged at `info` and treated
///   as `Ok(())`.
///
/// The reason `Locked` does not fail the parent operation: at this
/// point the client-instance registry has already been written to
/// disk, and refusing the parent would leave the operator with no
/// way to complete the remap/delete until the previous store is
/// unlocked — which may be intentional and operator-controlled. The
/// orphan rows are audit-visible and self-healing when the store
/// unlocks; surfacing them is the right trade-off.
pub(crate) fn drop_client_instance_permissions(
    state: &ListenerState,
    principal: &Principal,
    store_id: &str,
    client_instance_id: &str,
) -> Result<()> {
    use crate::store_registry::RegistryLookupError;
    let store = match state
        .shared
        .stores
        .read()
        .unwrap()
        .unlocked_backend(store_id)
    {
        Ok(s) => s,
        Err(RegistryLookupError::Locked) => {
            tracing::warn!(
                store = %store_id,
                client_instance = %client_instance_id,
                "store locked: wrapper permissions for reassigned/deleted client instance left in place; unlock the store to clean them up",
            );
            log_admin_event(
                "client-instance.orphan-permissions",
                principal,
                AuditEntity::ClientInstance {
                    id: client_instance_id,
                },
                Some("store-locked"),
                "skipped",
            );
            return Ok(());
        }
        Err(RegistryLookupError::NotFound) => {
            tracing::info!(
                store = %store_id,
                client_instance = %client_instance_id,
                "previous store gone: no policy cleanup needed for reassigned/deleted client instance",
            );
            return Ok(());
        }
    };
    let mut doc = load_policy_document(store.as_ref())?;
    let before = doc.permissions.len();
    doc.permissions
        .retain(|p| p.client_instance != client_instance_id);
    if doc.permissions.len() == before {
        return Ok(());
    }
    save_policy_document(store.as_ref(), &doc)
}

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<ListenerState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_wrapper_permissions_list))
        .routes(routes!(
            handle_admin_wrapper_permission_upsert,
            handle_admin_wrapper_permission_delete
        ))
}

#[utoipa::path(
    get,
    path = "/v1/admin/stores/{id}/wrapper-permissions",
    tag = "policy",
    security(("session_cookie" = [])),
    params(("id" = String, Path, description = "Store id.")),
    responses(
        (status = 200, description = "List of wrapper permissions for the store.", body = AdminWrapperPermissionsListResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 503, description = "Store is locked (`store_locked`).", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_wrapper_permissions_list(
    State(state): State<Arc<ListenerState>>,
    Path(store_id): Path<String>,
) -> Result<Json<AdminWrapperPermissionsListResponse>, AdminError> {
    let store = admin_store(&state, &store_id)?;
    let doc = load_policy_document(store.as_ref()).into_internal("loading policy document")?;
    Ok(Json(AdminWrapperPermissionsListResponse {
        permissions: doc.permissions,
    }))
}

#[utoipa::path(
    put,
    path = "/v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}",
    tag = "policy",
    security(("session_cookie" = [], "csrf_header" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("client_instance" = String, Path, description = "Client instance id."),
        ("wrapper" = String, Path, description = "Wrapper name."),
        ("rid" = String, Path, description = "Resource id."),
    ),
    request_body = AdminWrapperPermissionUpsertRequest,
    responses(
        (status = 200, description = "Permission upserted.", body = AdminOkResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Store, client instance, resource, or pair not found.", body = ServiceErrorResponse),
        (status = 503, description = "Store is locked (`store_locked`).", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_wrapper_permission_upsert(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, client_instance, wrapper, resource_id)): Path<(String, String, String, String)>,
    Body(body): Body<AdminWrapperPermissionUpsertRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    if client_instance.is_empty() || wrapper.is_empty() || resource_id.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_permission",
            "client_instance, wrapper, and resource_id must all be non-empty",
        ));
    }
    let permission = WrapperPermission {
        client_instance: client_instance.clone(),
        wrapper: wrapper.clone(),
        resource_id: resource_id.clone(),
        mode: body.mode,
        writable: body.writable,
        reason: body.reason,
    };
    let store = admin_store(&state, &store_id)?;
    require_client_instance_in_store(&state, &store_id, &client_instance)?;
    require_resource(store.as_ref(), &resource_id)?;
    require_pair(store.as_ref(), &wrapper, &resource_id)?;
    let mut doc = load_policy_document(store.as_ref()).into_internal("loading policy document")?;
    if let Some(slot) = doc.permissions.iter_mut().find(|p| {
        p.client_instance == client_instance && p.wrapper == wrapper && p.resource_id == resource_id
    }) {
        *slot = permission.clone();
    } else {
        doc.permissions.push(permission.clone());
    }
    doc.permissions.sort_by(|a, b| {
        a.client_instance
            .cmp(&b.client_instance)
            .then(a.wrapper.cmp(&b.wrapper))
            .then(a.resource_id.cmp(&b.resource_id))
    });
    if let Err(e) = save_policy_document(store.as_ref(), &doc) {
        log_admin_event(
            "wrapper_permission.upsert",
            &principal,
            AuditEntity::Permission {
                store: &store_id,
                permission: &permission,
            },
            None,
            "failure",
        );
        return Err(AdminError::internal("persisting policy document", e));
    }
    log_admin_event(
        "wrapper_permission.upsert",
        &principal,
        AuditEntity::Permission {
            store: &store_id,
            permission: &permission,
        },
        None,
        "success",
    );
    Ok(Json(AdminOkResponse { ok: true }))
}

#[utoipa::path(
    delete,
    path = "/v1/admin/stores/{id}/wrapper-permissions/{client_instance}/{wrapper}/{rid}",
    tag = "policy",
    security(("session_cookie" = [], "csrf_header" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("client_instance" = String, Path, description = "Client instance id."),
        ("wrapper" = String, Path, description = "Wrapper name."),
        ("rid" = String, Path, description = "Resource id."),
    ),
    responses(
        (status = 200, description = "Permission removed.", body = AdminOkResponse),
        (status = 404, description = "Store / permission not found.", body = ServiceErrorResponse),
        (status = 503, description = "Store is locked (`store_locked`).", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_wrapper_permission_delete(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, client_instance, wrapper, resource_id)): Path<(String, String, String, String)>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let store = admin_store(&state, &store_id)?;
    let mut doc = load_policy_document(store.as_ref()).into_internal("loading policy document")?;
    let before = doc.permissions.len();
    doc.permissions.retain(|p| {
        !(p.client_instance == client_instance
            && p.wrapper == wrapper
            && p.resource_id == resource_id)
    });
    let removed = before != doc.permissions.len();
    if !removed {
        return Err(AdminError::not_found("no matching wrapper permission"));
    }
    save_policy_document(store.as_ref(), &doc).into_internal("persisting policy document")?;
    let removed_permission = WrapperPermission {
        client_instance: client_instance.clone(),
        wrapper: wrapper.clone(),
        resource_id: resource_id.clone(),
        mode: underwrap_protocol::policy::WrapperApprovalMode::Locked,
        writable: false,
        reason: None,
    };
    log_admin_event(
        "wrapper_permission.delete",
        &principal,
        AuditEntity::Permission {
            store: &store_id,
            permission: &removed_permission,
        },
        None,
        "success",
    );
    Ok(Json(AdminOkResponse { ok: true }))
}

#[cfg(test)]
mod tests;
