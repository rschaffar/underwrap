//! Policy document + wrapper-permission admin CRUD.
//!
//! Two URL families share this module because they manipulate the same
//! backing document (`POLICY_DOCUMENT_SECRET_ID`):
//!
//! - `/v1/admin/policy/{get, put}` — read/write the whole policy
//!   document.
//! - `/v1/admin/wrapper-permissions/{list, upsert, delete}` — granular
//!   read-modify-write on the `permissions` vec.
//!
//! Plus `drop_client_instance_permissions`, which the client-instance
//! admin module calls when an instance is reassigned to a different
//! store.

use std::sync::Arc;

use anyhow::Result;
use axum::Json;
use axum::extract::State;
use underwrap_protocol::service::{
    AdminOkResponse, AdminPolicyGetRequest, AdminPolicyGetResponse,
    AdminWrapperPermissionDeleteRequest, AdminWrapperPermissionUpsertRequest,
    AdminWrapperPermissionsListRequest, AdminWrapperPermissionsListResponse, ServiceErrorResponse,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::audit::{AuditEntity, log_admin_event};
use crate::admin_api::stores::admin_store;
use crate::http::responses::Body;
use crate::runtime_state::AdminRuntimeState;
use crate::storage::documents::{load_policy_document, save_policy_document};
use crate::users::Principal;

/// Strip every wrapper permission whose `client_instance` matches
/// `client_instance_id` from the policy document of `store_id`. Used
/// when a client instance is reassigned to a different store, so its
/// authorizations on the previous store do not silently resurface if
/// it is ever remapped back.
pub(crate) fn drop_client_instance_permissions(
    state: &AdminRuntimeState,
    store_id: &str,
    client_instance_id: &str,
) -> Result<()> {
    let store = match state.stores.read().unwrap().unlocked_backend(store_id) {
        Ok(s) => s,
        // Skip when locked or unknown — the unlock path will be the
        // place to apply this kind of cleanup if it ever matters.
        Err(_) => return Ok(()),
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

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_policy_get))
        .routes(routes!(handle_admin_wrapper_permissions_list))
        .routes(routes!(handle_admin_wrapper_permission_upsert))
        .routes(routes!(handle_admin_wrapper_permission_delete))
}

#[utoipa::path(
    post,
    path = "/v1/admin/policy/get",
    tag = "policy",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminPolicyGetRequest,
    responses(
        (status = 200, description = "Policy document for the store.", body = AdminPolicyGetResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_policy_get(
    State(state): State<Arc<AdminRuntimeState>>,
    Body(body): Body<AdminPolicyGetRequest>,
) -> Result<Json<AdminPolicyGetResponse>, AdminError> {
    let store = admin_store(&state, &body.store)?;
    let document = load_policy_document(store.as_ref()).map_err(AdminError::admin)?;
    Ok(Json(AdminPolicyGetResponse { document }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/wrapper-permissions/list",
    tag = "policy",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminWrapperPermissionsListRequest,
    responses(
        (status = 200, description = "List of wrapper permissions for the store.", body = AdminWrapperPermissionsListResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_wrapper_permissions_list(
    State(state): State<Arc<AdminRuntimeState>>,
    Body(body): Body<AdminWrapperPermissionsListRequest>,
) -> Result<Json<AdminWrapperPermissionsListResponse>, AdminError> {
    let store = admin_store(&state, &body.store)?;
    let doc = load_policy_document(store.as_ref()).map_err(AdminError::admin)?;
    Ok(Json(AdminWrapperPermissionsListResponse {
        permissions: doc.permissions,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/wrapper-permissions/upsert",
    tag = "policy",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminWrapperPermissionUpsertRequest,
    responses(
        (status = 200, description = "Permission upserted.", body = AdminOkResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Store / wrapper / resource not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_wrapper_permission_upsert(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminWrapperPermissionUpsertRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let store = admin_store(&state, &body.store)?;
    if body.permission.client_instance.is_empty()
        || body.permission.wrapper.is_empty()
        || body.permission.resource_id.is_empty()
    {
        return Err(AdminError::bad_request(
            "invalid_permission",
            "client_instance, wrapper, and resource_id must all be non-empty",
        ));
    }
    let mut doc = load_policy_document(store.as_ref()).map_err(AdminError::admin)?;
    if let Some(slot) = doc.permissions.iter_mut().find(|p| {
        p.client_instance == body.permission.client_instance
            && p.wrapper == body.permission.wrapper
            && p.resource_id == body.permission.resource_id
    }) {
        *slot = body.permission.clone();
    } else {
        doc.permissions.push(body.permission.clone());
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
                store: &body.store,
                permission: &body.permission,
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
            store: &body.store,
            permission: &body.permission,
        },
        None,
        "success",
    );
    Ok(Json(AdminOkResponse { ok: true }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/wrapper-permissions/delete",
    tag = "policy",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminWrapperPermissionDeleteRequest,
    responses(
        (status = 200, description = "Permission removed.", body = AdminOkResponse),
        (status = 404, description = "Store / permission not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_wrapper_permission_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminWrapperPermissionDeleteRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let store = admin_store(&state, &body.store)?;
    let mut doc = load_policy_document(store.as_ref()).map_err(AdminError::admin)?;
    let before = doc.permissions.len();
    doc.permissions.retain(|p| {
        !(p.client_instance == body.client_instance
            && p.wrapper == body.wrapper
            && p.resource_id == body.resource_id)
    });
    let removed = before != doc.permissions.len();
    if !removed {
        return Err(AdminError::not_found("no matching wrapper permission"));
    }
    save_policy_document(store.as_ref(), &doc)
        .map_err(|e| AdminError::internal("persisting policy document", e))?;
    let removed_permission = underwrap_protocol::policy::WrapperPermission {
        client_instance: body.client_instance.clone(),
        wrapper: body.wrapper.clone(),
        resource_id: body.resource_id.clone(),
        mode: underwrap_protocol::policy::WrapperApprovalMode::Locked,
        writable: false,
        reason: None,
    };
    log_admin_event(
        "wrapper_permission.delete",
        &principal,
        AuditEntity::Permission {
            store: &body.store,
            permission: &removed_permission,
        },
        None,
        "success",
    );
    Ok(Json(AdminOkResponse { ok: true }))
}
