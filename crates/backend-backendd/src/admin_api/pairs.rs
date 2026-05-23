//! Wrapper / resource pair admin CRUD:
//! `/v1/admin/pairs/{list, create, delete}`.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use underwrap_protocol::service::{
    AdminPairCreateRequest, AdminPairCreateResponse, AdminPairDeleteRequest,
    AdminPairDeleteResponse, AdminPairsListRequest, AdminPairsListResponse, ServiceErrorResponse,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::audit::{AuditEntity, log_admin_event};
use crate::admin_api::stores::admin_store;
use crate::http::responses::Body;
use crate::runtime_state::AdminRuntimeState;
use crate::storage::documents::{
    ENTITY_DEPENDENT_SAMPLE_LIMIT, load_pairs_document, load_policy_document,
    load_resources_document, save_pairs_document,
};
use crate::users::Principal;

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_pairs_list))
        .routes(routes!(handle_admin_pair_create))
        .routes(routes!(handle_admin_pair_delete))
}

#[utoipa::path(
    post,
    path = "/v1/admin/pairs/list",
    tag = "pairs",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminPairsListRequest,
    responses(
        (status = 200, description = "Wrapper / resource pairs in the store.", body = AdminPairsListResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_pairs_list(
    State(state): State<Arc<AdminRuntimeState>>,
    Body(body): Body<AdminPairsListRequest>,
) -> Result<Json<AdminPairsListResponse>, AdminError> {
    let store = admin_store(&state, &body.store)?;
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
    path = "/v1/admin/pairs/create",
    tag = "pairs",
    security(("session_cookie" = []), ("csrf_header" = [])),
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
    let store = admin_store(&state, &body.store)?;
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
    path = "/v1/admin/pairs/delete",
    tag = "pairs",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminPairDeleteRequest,
    responses(
        (status = 200, description = "Pair removed.", body = AdminPairDeleteResponse),
        (status = 400, description = "Pair still referenced by wrapper permissions — see the `blocked_by` field.", body = AdminPairDeleteResponse),
        (status = 404, description = "Store / pair not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_pair_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminPairDeleteRequest>,
) -> Result<Json<AdminPairDeleteResponse>, AdminError> {
    use underwrap_protocol::service::{AdminPairDeleteBlockers, AdminWrapperPermissionKey};

    let store = admin_store(&state, &body.store)?;
    let mut doc = load_pairs_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading pairs document", e))?;
    if doc.find(&body.wrapper, &body.resource_id).is_none() {
        return Err(AdminError::not_found("unknown pair"));
    }

    let policy = load_policy_document(store.as_ref())
        .map_err(|e| AdminError::internal("loading policy document", e))?;
    let dependent_permissions: Vec<AdminWrapperPermissionKey> = policy
        .permissions
        .iter()
        .filter(|p| p.wrapper == body.wrapper && p.resource_id == body.resource_id)
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
                wrapper: &body.wrapper,
                resource_id: &body.resource_id,
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

    doc.remove(&body.wrapper, &body.resource_id);
    save_pairs_document(store.as_ref(), &doc)
        .map_err(|e| AdminError::internal("persisting pairs document", e))?;
    log_admin_event(
        "pair.delete",
        &principal,
        AuditEntity::Pair {
            wrapper: &body.wrapper,
            resource_id: &body.resource_id,
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
