//! Resource admin CRUD under `/v1/admin/stores/{id}/resources`.
//!
//! Resources are first-class entities with provenance, stored in the
//! resources document. Delete refuses with structured blockers if stored
//! resource content, pairs, or wrapper permissions still reference the
//! resource.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use underwrap_backend_store::delete_resource_blob;
use underwrap_protocol::entities::{EntityCreator, InsertError, ResourceRecord};
use underwrap_protocol::service::{
    AdminResourceCreateRequest, AdminResourceCreateResponse, AdminResourceDeleteBlockers,
    AdminResourceDeleteResponse, AdminResourceRenameRequest, AdminResourceRenameResponse,
    AdminResourcesListResponse, ServiceErrorResponse, ServiceWrapperResourcePair,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::audit::{AuditEntity, log_admin_event};
use crate::admin_api::csrf::Principal;
use crate::admin_api::stores::admin_store;
use crate::admin_api::util::IntoInternal;
use crate::http::responses::Body;

use crate::runtime_state::ListenerState;
use crate::storage::documents::{
    ENTITY_DEPENDENT_SAMPLE_LIMIT, load_pairs_document, load_resources_document,
    policy_permission_dependents, resource_content_dependents, save_resources_document,
};

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<ListenerState>> {
    OpenApiRouter::new()
        .routes(routes!(
            handle_admin_resources_list,
            handle_admin_resource_create
        ))
        .routes(routes!(
            handle_admin_resource_rename,
            handle_admin_resource_delete
        ))
}

#[utoipa::path(
    get,
    path = "/v1/admin/stores/{id}/resources",
    tag = "resources",
    security(("session_cookie" = [])),
    params(("id" = String, Path, description = "Store id.")),
    responses(
        (status = 200, description = "Resources in the store.", body = AdminResourcesListResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_resources_list(
    State(state): State<Arc<ListenerState>>,
    Path(store_id): Path<String>,
) -> Result<Json<AdminResourcesListResponse>, AdminError> {
    let store = admin_store(&state, &store_id)?;
    let mut doc =
        load_resources_document(store.as_ref()).into_internal("loading resources document")?;
    doc.resources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(AdminResourcesListResponse {
        resources: doc.resources,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/stores/{id}/resources",
    tag = "resources",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
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
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(store_id): Path<String>,
    Body(body): Body<AdminResourceCreateRequest>,
) -> Result<Json<AdminResourceCreateResponse>, AdminError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_resource_name",
            "resource name must be non-empty",
        ));
    }
    let store = admin_store(&state, &store_id)?;
    let mut doc =
        load_resources_document(store.as_ref()).into_internal("loading resources document")?;
    if doc.find_by_name(name).is_some() {
        return Err(AdminError::conflict(
            "resource_already_exists",
            format!("resource '{name}' already exists"),
        ));
    }
    let record = ResourceRecord {
        id: crate::ids::random_id("res"),
        name: name.to_string(),
        created_by: EntityCreator::Operator {
            id: principal.id.clone(),
            username: principal.username.clone(),
        },
        created_at: crate::time::now_epoch_secs(),
    };
    doc.insert(record.clone())
        .expect("find returned None just above");
    save_resources_document(store.as_ref(), &doc).into_internal("persisting resources document")?;
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
    delete,
    path = "/v1/admin/stores/{id}/resources/{rid}",
    tag = "resources",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
    ),
    responses(
        (status = 200, description = "Resource deleted, or `ok: false` with structured blockers.", body = AdminResourceDeleteResponse),
        (status = 404, description = "Store / resource not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_resource_delete(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id)): Path<(String, String)>,
) -> Result<Json<AdminResourceDeleteResponse>, AdminError> {
    let store = admin_store(&state, &store_id)?;
    let mut doc =
        load_resources_document(store.as_ref()).into_internal("loading resources document")?;
    let Some(target) = doc.find_by_id(&resource_id) else {
        return Err(AdminError::resource_not_found());
    };
    let target_name = target.name.clone();

    let (content_entry_count, sample_content) =
        resource_content_dependents(store.as_ref(), &resource_id)
            .into_internal("scanning resource content")?;

    let pairs = load_pairs_document(store.as_ref()).into_internal("loading pairs document")?;
    let dependent_pairs: Vec<ServiceWrapperResourcePair> = pairs
        .pairs_for_resource(&resource_id)
        .map(|p| ServiceWrapperResourcePair {
            wrapper: p.wrapper.clone(),
            resource: target_name.clone(),
        })
        .collect();
    let pair_count = crate::admin_api::util::saturating_u32(dependent_pairs.len());
    let mut sample_pairs = dependent_pairs;
    sample_pairs.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);

    let (permission_count, sample_permissions) =
        policy_permission_dependents(store.as_ref(), |p| p.resource_id == resource_id)
            .into_internal("loading policy document")?;

    if content_entry_count > 0 || pair_count > 0 || permission_count > 0 {
        log_admin_event(
            "resource.delete",
            &principal,
            AuditEntity::Resource {
                resource_id: &resource_id,
            },
            None,
            "blocked",
        );
        return Ok(Json(AdminResourceDeleteResponse {
            ok: false,
            blocked_by: Some(AdminResourceDeleteBlockers {
                content_entry_count,
                sample_content,
                pair_count,
                sample_pairs,
                permission_count,
                sample_permissions,
            }),
        }));
    }

    // Remove the active empty/missing blob key before making the catalog
    // record disappear. `Ok(false)` is the valid never-materialized case;
    // any returned error stops before the catalog write.
    let _ = delete_resource_blob(store.as_ref(), &resource_id)
        .into_internal("deleting resource blob")?;
    doc.resources.retain(|resource| resource.id != resource_id);
    save_resources_document(store.as_ref(), &doc).into_internal("persisting resources document")?;
    log_admin_event(
        "resource.delete",
        &principal,
        AuditEntity::Resource {
            resource_id: &resource_id,
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
    put,
    path = "/v1/admin/stores/{id}/resources/{rid}",
    tag = "resources",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
    ),
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
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id)): Path<(String, String)>,
    Body(body): Body<AdminResourceRenameRequest>,
) -> Result<Json<AdminResourceRenameResponse>, AdminError> {
    let new_name = body.new_name.trim();
    if new_name.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_resource_name",
            "new_name must be non-empty",
        ));
    }
    let store = admin_store(&state, &store_id)?;
    let mut doc =
        load_resources_document(store.as_ref()).into_internal("loading resources document")?;
    match doc.rename(&resource_id, new_name) {
        Ok(true) => {}
        Ok(false) => return Err(AdminError::resource_not_found()),
        Err(InsertError::Duplicate) => {
            return Err(AdminError::conflict(
                "resource_already_exists",
                format!("resource name '{new_name}' is already taken"),
            ));
        }
    }
    let updated = doc
        .find_by_id(&resource_id)
        .expect("rename above returned Ok(true)")
        .clone();
    save_resources_document(store.as_ref(), &doc).into_internal("persisting resources document")?;
    log_admin_event(
        "resource.rename",
        &principal,
        AuditEntity::Resource {
            resource_id: &resource_id,
        },
        None,
        "success",
    );
    Ok(Json(AdminResourceRenameResponse { resource: updated }))
}

#[cfg(test)]
mod tests;
