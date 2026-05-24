//! Wrapper / resource pair admin CRUD under
//! `/v1/admin/stores/{id}/pairs`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use underwrap_protocol::service::{
    AdminPairCreateRequest, AdminPairCreateResponse, AdminPairDeleteResponse,
    AdminPairWithNewResourceRequest, AdminPairWithNewResourceResponse, AdminPairsListResponse,
    ServiceErrorResponse,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::audit::{AuditEntity, log_admin_event};
use crate::admin_api::stores::admin_store;
use crate::http::responses::Body;
use crate::runtime_state::AdminRuntimeState;
use crate::runtime_api::wrappers::generate_resource_id;
use crate::storage::documents::{
    ENTITY_DEPENDENT_SAMPLE_LIMIT, load_pairs_document, load_policy_document,
    load_resources_document, save_pairs_document, save_resources_document,
};
use crate::users::Principal;

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_pairs_list, handle_admin_pair_create))
        .routes(routes!(handle_admin_pair_with_new_resource))
        .routes(routes!(handle_admin_pair_delete))
}

#[utoipa::path(
    get,
    path = "/v1/admin/stores/{id}/pairs",
    tag = "pairs",
    security(("session_cookie" = [])),
    params(("id" = String, Path, description = "Store id.")),
    responses(
        (status = 200, description = "Wrapper / resource pairs in the store.", body = AdminPairsListResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_pairs_list(
    State(state): State<Arc<AdminRuntimeState>>,
    Path(store_id): Path<String>,
) -> Result<Json<AdminPairsListResponse>, AdminError> {
    let store = admin_store(&state, &store_id)?;
    let mut doc = load_pairs_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading pairs document", e))?;
    doc.pairs.sort_by(|a, b| {
        a.wrapper
            .cmp(&b.wrapper)
            .then(a.resource_id.cmp(&b.resource_id))
    });
    Ok(Json(AdminPairsListResponse { pairs: doc.pairs }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/stores/{id}/pairs",
    tag = "pairs",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    request_body = AdminPairCreateRequest,
    responses(
        (status = 200, description = "Pair created.", body = AdminPairCreateResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Store / wrapper / resource not found.", body = ServiceErrorResponse),
        (status = 409, description = "Pair already exists.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_pair_create(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(store_id): Path<String>,
    Body(body): Body<AdminPairCreateRequest>,
) -> Result<Json<AdminPairCreateResponse>, AdminError> {
    use underwrap_protocol::entities::{EntityCreator, WrapperResourcePairRecord};

    let wrapper = body.wrapper.trim();
    let resource_id = body.resource_id.trim();
    if wrapper.is_empty() || resource_id.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_pair",
            "wrapper and resource_id must both be non-empty",
        ));
    }
    let store = admin_store(&state, &store_id)?;
    let resources = load_resources_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading resources document", e))?;
    if resources.find_by_id(resource_id).is_none() {
        return Err(AdminError::bad_request(
            "unknown_resource",
            format!("resource id '{resource_id}' must exist before a pair can reference it"),
        ));
    }
    let mut pairs = load_pairs_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading pairs document", e))?;
    if pairs.find(wrapper, resource_id).is_some() {
        return Err(AdminError::conflict(
            "pair_already_exists",
            format!("pair ({wrapper}, {resource_id}) already exists"),
        ));
    }
    let record = WrapperResourcePairRecord {
        wrapper: wrapper.to_string(),
        resource_id: resource_id.to_string(),
        created_by: EntityCreator::Operator {
            id: principal.id.clone(),
            username: principal.username.clone(),
        },
        created_at: crate::users::now_epoch_secs(),
        last_used_at: None,
    };
    pairs
        .insert(record.clone())
        .expect("find returned None just above");
    save_pairs_document(store.as_ref(), &pairs)
        .map_err(|e| AdminError::internal("persisting pairs document", e))?;
    log_admin_event(
        "pair.create",
        &principal,
        AuditEntity::Pair {
            wrapper,
            resource_id,
        },
        None,
        "success",
    );
    Ok(Json(AdminPairCreateResponse { pair: record }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/stores/{id}/pairs/with-new-resource",
    tag = "pairs",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    request_body = AdminPairWithNewResourceRequest,
    responses(
        (status = 200, description = "Resource created and paired with the wrapper in one step.", body = AdminPairWithNewResourceResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 409, description = "Resource name already exists.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_pair_with_new_resource(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(store_id): Path<String>,
    Body(body): Body<AdminPairWithNewResourceRequest>,
) -> Result<Json<AdminPairWithNewResourceResponse>, AdminError> {
    use underwrap_protocol::entities::{EntityCreator, ResourceRecord, WrapperResourcePairRecord};

    let wrapper = body.wrapper.trim();
    let resource_name = body.resource_name.trim();
    if wrapper.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_pair",
            "wrapper must be non-empty",
        ));
    }
    if resource_name.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_resource_name",
            "resource name must be non-empty",
        ));
    }
    let store = admin_store(&state, &store_id)?;

    let mut res_doc = load_resources_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading resources document", e))?;
    if res_doc.find_by_name(resource_name).is_some() {
        return Err(AdminError::conflict(
            "resource_already_exists",
            format!("resource '{resource_name}' already exists"),
        ));
    }
    let original_resources = res_doc.clone();
    let resource = ResourceRecord {
        id: generate_resource_id(),
        name: resource_name.to_string(),
        created_by: EntityCreator::Operator {
            id: principal.id.clone(),
            username: principal.username.clone(),
        },
        created_at: crate::users::now_epoch_secs(),
    };
    res_doc
        .insert(resource.clone())
        .expect("find_by_name returned None and id is freshly generated");

    let mut pairs_doc = load_pairs_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading pairs document", e))?;
    let pair = WrapperResourcePairRecord {
        wrapper: wrapper.to_string(),
        resource_id: resource.id.clone(),
        created_by: EntityCreator::Operator {
            id: principal.id.clone(),
            username: principal.username.clone(),
        },
        created_at: crate::users::now_epoch_secs(),
        last_used_at: None,
    };
    pairs_doc
        .insert(pair.clone())
        .expect("freshly generated resource id cannot collide with existing pairs");

    save_resources_document(store.as_ref(), &res_doc)
        .map_err(|e| AdminError::internal("persisting resources document", e))?;
    if let Err(e) = save_pairs_document(store.as_ref(), &pairs_doc) {
        // Best-effort rollback so the resource doesn't dangle without
        // its companion pair. If even the rollback fails, the operator
        // sees the original error; the audit trail still has the
        // success of the first save vs. the failure of the second.
        if let Err(rollback_err) = save_resources_document(store.as_ref(), &original_resources) {
            tracing::warn!(
                err = format!("{rollback_err:#}"),
                "pair_with_new_resource: rollback of resources document failed; resource may be orphaned",
            );
        }
        return Err(AdminError::internal("persisting pairs document", e));
    }

    log_admin_event(
        "resource.create",
        &principal,
        AuditEntity::Resource {
            resource_id: &resource.id,
        },
        None,
        "success",
    );
    log_admin_event(
        "pair.create",
        &principal,
        AuditEntity::Pair {
            wrapper,
            resource_id: &resource.id,
        },
        None,
        "success",
    );
    Ok(Json(AdminPairWithNewResourceResponse { resource, pair }))
}

#[utoipa::path(
    delete,
    path = "/v1/admin/stores/{id}/pairs/{wrapper}/{rid}",
    tag = "pairs",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("wrapper" = String, Path, description = "Wrapper name."),
        ("rid" = String, Path, description = "Resource id."),
    ),
    responses(
        (status = 200, description = "Pair removed, or `ok: false` with structured blockers.", body = AdminPairDeleteResponse),
        (status = 404, description = "Store / pair not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_pair_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, wrapper, resource_id)): Path<(String, String, String)>,
) -> Result<Json<AdminPairDeleteResponse>, AdminError> {
    use underwrap_protocol::service::{AdminPairDeleteBlockers, AdminWrapperPermissionKey};

    let store = admin_store(&state, &store_id)?;
    let mut doc = load_pairs_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading pairs document", e))?;
    if doc.find(&wrapper, &resource_id).is_none() {
        return Err(AdminError::not_found("unknown pair"));
    }

    let policy = load_policy_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading policy document", e))?;
    let dependent_permissions: Vec<AdminWrapperPermissionKey> = policy
        .permissions
        .iter()
        .filter(|p| p.wrapper == wrapper && p.resource_id == resource_id)
        .map(|p| AdminWrapperPermissionKey {
            client_instance: p.client_instance.clone(),
            wrapper: p.wrapper.clone(),
            resource_id: p.resource_id.clone(),
        })
        .collect();
    let permission_count = u32::try_from(dependent_permissions.len()).unwrap_or(u32::MAX);
    let mut sample_permissions = dependent_permissions;
    sample_permissions.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);

    if permission_count > 0 {
        log_admin_event(
            "pair.delete",
            &principal,
            AuditEntity::Pair {
                wrapper: &wrapper,
                resource_id: &resource_id,
            },
            None,
            "blocked",
        );
        return Ok(Json(AdminPairDeleteResponse {
            ok: false,
            blocked_by: Some(AdminPairDeleteBlockers {
                permission_count,
                sample_permissions,
            }),
        }));
    }

    doc.remove(&wrapper, &resource_id);
    save_pairs_document(store.as_ref(), &doc)
        .map_err(|e| AdminError::internal("persisting pairs document", e))?;
    log_admin_event(
        "pair.delete",
        &principal,
        AuditEntity::Pair {
            wrapper: &wrapper,
            resource_id: &resource_id,
        },
        None,
        "success",
    );
    Ok(Json(AdminPairDeleteResponse {
        ok: true,
        blocked_by: None,
    }))
}

#[cfg(test)]
mod tests;
