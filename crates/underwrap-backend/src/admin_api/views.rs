//! Aggregate read-only views for SPA pages that need several
//! per-store documents in one shot. Both endpoints are pure
//! server-side joins of documents already in the store; they add no
//! new authorisation surface — the admin store-resolver
//! (`admin_store`) is the same gate the per-document handlers go
//! through.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use underwrap_protocol::service::{
    AdminSecretsView, AdminWrapperPermissionsView, ServiceErrorResponse,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::stores::admin_store;
use crate::admin_api::util::IntoInternal;
use crate::runtime_state::ListenerState;
use crate::storage::documents::{
    load_pairs_document, load_policy_document, load_resources_document,
};
use crate::storage::secret_entries::list_secret_entries;

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<ListenerState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_wrapper_permissions_view))
        .routes(routes!(handle_admin_secrets_view))
}

#[utoipa::path(
    get,
    path = "/v1/admin/stores/{id}/wrapper-permissions-view",
    tag = "policy",
    security(("session_cookie" = [])),
    params(("id" = String, Path, description = "Store id.")),
    responses(
        (status = 200, description = "Permissions, pairs, and resources for the store, in one round trip.", body = AdminWrapperPermissionsView),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_wrapper_permissions_view(
    State(state): State<Arc<ListenerState>>,
    Path(store_id): Path<String>,
) -> Result<Json<AdminWrapperPermissionsView>, AdminError> {
    let store = admin_store(&state, &store_id)?;
    let policy = load_policy_document(store.as_ref()).into_internal("loading policy document")?;
    let mut pairs = load_pairs_document(store.as_ref()).into_internal("loading pairs document")?;
    pairs.pairs.sort_by(|a, b| {
        a.wrapper
            .cmp(&b.wrapper)
            .then(a.resource_id.cmp(&b.resource_id))
    });
    let mut resources =
        load_resources_document(store.as_ref()).into_internal("loading resources document")?;
    resources.resources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(AdminWrapperPermissionsView {
        permissions: policy.permissions,
        pairs: pairs.pairs,
        resources: resources.resources,
    }))
}

#[utoipa::path(
    get,
    path = "/v1/admin/stores/{id}/secrets-view",
    tag = "secrets",
    security(("session_cookie" = [])),
    params(("id" = String, Path, description = "Store id.")),
    responses(
        (status = 200, description = "Secret entries and resources for the store, in one round trip.", body = AdminSecretsView),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_secrets_view(
    State(state): State<Arc<ListenerState>>,
    Path(store_id): Path<String>,
) -> Result<Json<AdminSecretsView>, AdminError> {
    let store = admin_store(&state, &store_id)?;
    let entries =
        list_secret_entries(store.as_ref(), None).into_internal("listing secret entries")?;
    let mut resources =
        load_resources_document(store.as_ref()).into_internal("loading resources document")?;
    resources.resources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(AdminSecretsView {
        entries,
        resources: resources.resources,
    }))
}
