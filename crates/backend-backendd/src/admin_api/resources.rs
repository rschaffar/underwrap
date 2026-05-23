//! Resource admin CRUD: `/v1/admin/resources/{list, create, delete,
//! rename}`.
//!
//! Resources are first-class entities with provenance, stored in the
//! resources document. Delete refuses with structured blockers if
//! secrets, pairs, or wrapper permissions still reference the
//! resource.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use underwrap_protocol::service::{
    AdminResourceCreateRequest, AdminResourceCreateResponse, AdminResourceDeleteRequest,
    AdminResourceDeleteResponse, AdminResourceRenameRequest, AdminResourceRenameResponse,
    AdminResourcesListRequest, AdminResourcesListResponse, ServiceErrorResponse,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::audit::{AuditEntity, log_admin_event};
use crate::admin_api::stores::admin_store;
use crate::http::responses::Body;
use crate::runtime_api::wrappers::generate_resource_id;
use crate::runtime_state::AdminRuntimeState;
use crate::storage::documents::{
    ENTITY_DEPENDENT_SAMPLE_LIMIT, load_pairs_document, load_policy_document,
    load_resources_document, resource_secret_dependents, save_resources_document,
};
use crate::users::Principal;

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_resources_list))
        .routes(routes!(handle_admin_resource_create))
        .routes(routes!(handle_admin_resource_delete))
        .routes(routes!(handle_admin_resource_rename))
}

#[utoipa::path(
    post,
    path = "/v1/admin/resources/list",
    tag = "resources",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminResourcesListRequest,
    responses(
        (status = 200, description = "Resources in the store.", body = AdminResourcesListResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_resources_list(
    State(state): State<Arc<AdminRuntimeState>>,
    Body(body): Body<AdminResourcesListRequest>,
) -> Result<Json<AdminResourcesListResponse>, AdminError> {
    let store = admin_store(&state, &body.store)?;
    let mut doc = load_resources_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading resources document", e))?;
    doc.resources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(AdminResourcesListResponse {
        resources: doc.resources,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/resources/create",
    tag = "resources",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminResourceCreateRequest,
    responses(
        (status = 200, description = "Resource created.", body = AdminResourceCreateResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 409, description = "Resource name already exists.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_resource_create(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminResourceCreateRequest>,
) -> Result<Json<AdminResourceCreateResponse>, AdminError> {
    use underwrap_protocol::entities::{EntityCreator, ResourceRecord};

    let name = body.name.trim();
    if name.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_resource_name",
            "resource name must be non-empty",
        ));
    }
    let store = admin_store(&state, &body.store)?;
    let mut doc = load_resources_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading resources document", e))?;
    if doc.find_by_name(name).is_some() {
        return Err(AdminError::conflict(
            "resource_already_exists",
            format!("resource '{name}' already exists"),
        ));
    }
    let record = ResourceRecord {
        id: generate_resource_id(),
        name: name.to_string(),
        created_by: EntityCreator::Operator {
            id: principal.id.clone(),
            username: principal.username.clone(),
        },
        created_at: crate::users::now_epoch_secs(),
    };
    doc.insert(record.clone())
        .expect("find returned None just above");
    save_resources_document(store.as_ref(), &doc)
        .map_err(|e| AdminError::internal("persisting resources document", e))?;
    log_admin_event(
        "resource.create",
        &principal,
        AuditEntity::Resource {
            resource_id: &record.id,
        },
        None,
        "success",
    );
    Ok(Json(AdminResourceCreateResponse { resource: record }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/resources/delete",
    tag = "resources",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminResourceDeleteRequest,
    responses(
        (status = 200, description = "Resource deleted.", body = AdminResourceDeleteResponse),
        (status = 400, description = "Resource still referenced by pairs / permissions / secrets — see the `blockers` field.", body = AdminResourceDeleteResponse),
        (status = 404, description = "Store / resource not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_resource_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminResourceDeleteRequest>,
) -> Result<Json<AdminResourceDeleteResponse>, AdminError> {
    use underwrap_protocol::service::{
        AdminResourceDeleteBlockers, AdminWrapperPermissionKey, ServiceWrapperResourcePair,
    };

    let store = admin_store(&state, &body.store)?;
    let mut doc = load_resources_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading resources document", e))?;
    let Some(target) = doc.find_by_id(&body.id) else {
        return Err(AdminError::not_found("unknown resource"));
    };
    let target_name = target.name.clone();

    let (secret_count, sample_secret_ids) = resource_secret_dependents(store.as_ref(), &body.id)
        .map_err(|e| AdminError::internal("scanning resource dependents", e))?;

    let pairs = load_pairs_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading pairs document", e))?;
    // The wire shape for blocker pairs still carries the *name* in
    // `resource` so the operator gets a readable label; resolve via the
    // resources document.
    let dependent_pairs: Vec<ServiceWrapperResourcePair> = pairs
        .pairs_for_resource(&body.id)
        .map(|p| ServiceWrapperResourcePair {
            wrapper: p.wrapper.clone(),
            resource: target_name.clone(),
        })
        .collect();
    let pair_count = u32::try_from(dependent_pairs.len()).unwrap_or(u32::MAX);
    let mut sample_pairs = dependent_pairs;
    sample_pairs.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);

    let policy = load_policy_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading policy document", e))?;
    let dependent_permissions: Vec<AdminWrapperPermissionKey> = policy
        .permissions
        .iter()
        .filter(|p| p.resource_id == body.id)
        .map(|p| AdminWrapperPermissionKey {
            client_instance: p.client_instance.clone(),
            wrapper: p.wrapper.clone(),
            resource_id: p.resource_id.clone(),
        })
        .collect();
    let permission_count = u32::try_from(dependent_permissions.len()).unwrap_or(u32::MAX);
    let mut sample_permissions = dependent_permissions;
    sample_permissions.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);

    if secret_count > 0 || pair_count > 0 || permission_count > 0 {
        log_admin_event(
            "resource.delete",
            &principal,
            AuditEntity::Resource {
                resource_id: &body.id,
            },
            None,
            "blocked",
        );
        return Ok(Json(AdminResourceDeleteResponse {
            ok: false,
            blocked_by: Some(AdminResourceDeleteBlockers {
                secret_count,
                sample_secret_ids,
                pair_count,
                sample_pairs,
                permission_count,
                sample_permissions,
            }),
        }));
    }

    doc.remove(&body.id);
    save_resources_document(store.as_ref(), &doc)
        .map_err(|e| AdminError::internal("persisting resources document", e))?;
    log_admin_event(
        "resource.delete",
        &principal,
        AuditEntity::Resource {
            resource_id: &body.id,
        },
        None,
        "success",
    );
    Ok(Json(AdminResourceDeleteResponse {
        ok: true,
        blocked_by: None,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/resources/rename",
    tag = "resources",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminResourceRenameRequest,
    responses(
        (status = 200, description = "Resource renamed.", body = AdminResourceRenameResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Store / resource not found.", body = ServiceErrorResponse),
        (status = 409, description = "Target name already exists.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_resource_rename(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminResourceRenameRequest>,
) -> Result<Json<AdminResourceRenameResponse>, AdminError> {
    use underwrap_protocol::entities::InsertError;

    let new_name = body.new_name.trim();
    if new_name.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_resource_name",
            "new_name must be non-empty",
        ));
    }
    let store = admin_store(&state, &body.store)?;
    let mut doc = load_resources_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading resources document", e))?;
    match doc.rename(&body.id, new_name) {
        Ok(true) => {}
        Ok(false) => return Err(AdminError::not_found("unknown resource")),
        Err(InsertError::Duplicate) => {
            return Err(AdminError::conflict(
                "resource_already_exists",
                format!("resource name '{new_name}' is already taken"),
            ));
        }
    }
    let updated = doc
        .find_by_id(&body.id)
        .expect("rename above returned Ok(true)")
        .clone();
    save_resources_document(store.as_ref(), &doc)
        .map_err(|e| AdminError::internal("persisting resources document", e))?;
    log_admin_event(
        "resource.rename",
        &principal,
        AuditEntity::Resource {
            resource_id: &body.id,
        },
        None,
        "success",
    );
    Ok(Json(AdminResourceRenameResponse { resource: updated }))
}

#[cfg(test)]
mod tests;
