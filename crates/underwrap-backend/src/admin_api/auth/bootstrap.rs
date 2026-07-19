//! Read-only first-operator setup status surface:
//! `GET /v1/admin/auth/bootstrap/status`.
//!
//! Initial operator creation is deliberately absent from the network API. The
//! SPA uses this status to direct a deployment operator to the quiesced offline
//! `underwrap-backend admin bootstrap-user` command.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::runtime_state::ListenerState;

#[derive(serde::Serialize, utoipa::ToSchema)]
pub(crate) struct BootstrapStatusResponse {
    pub bootstrap_required: bool,
}

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<ListenerState>> {
    OpenApiRouter::new().routes(routes!(handle_admin_bootstrap_status))
}

/// `GET /v1/admin/auth/bootstrap/status`. Returns setup-required only when the
/// sentinel is absent and the operator collection loads successfully as empty.
/// Every read or decode error fails closed to `false`.
#[utoipa::path(
    get,
    path = "/v1/admin/auth/bootstrap/status",
    tag = "bootstrap",
    responses(
        (status = 200, description = "True only when the operator collection is empty and the bootstrap-completed sentinel is unset.", body = BootstrapStatusResponse),
    ),
)]
pub(crate) async fn handle_admin_bootstrap_status(
    State(state): State<Arc<ListenerState>>,
) -> Json<BootstrapStatusResponse> {
    let sentinel_absent = crate::users::bootstrap_completed(&state.admin().bootstrap_file)
        .map(|completed| !completed)
        .unwrap_or(false);
    let collection_empty = crate::users::load(&state.admin().users_dir)
        .map(|collection| collection.is_empty())
        .unwrap_or(false);
    Json(BootstrapStatusResponse {
        bootstrap_required: sentinel_absent && collection_empty,
    })
}

#[cfg(test)]
mod tests;
