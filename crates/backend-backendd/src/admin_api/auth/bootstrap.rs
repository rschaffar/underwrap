//! First-operator bootstrap surface:
//! `GET /v1/admin/auth/bootstrap/status` and
//! `POST /v1/admin/auth/bootstrap`.
//!
//! The status endpoint is unconditional — the SPA pings it to choose
//! between the login page and the bootstrap page. The bootstrap
//! endpoint creates the very first admin operator, marks the
//! persistent sentinel, and issues a session in one round trip.
//! After this succeeds once, further calls are refused even if every
//! operator is later deleted.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use tracing::{info, warn};
use underwrap_protocol::service::ServiceErrorResponse;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::audit::log_bootstrap_attempt;

use crate::admin_api::auth::AdminAuthSession;
use crate::http::responses::Body;
use crate::runtime_state::AdminRuntimeState;

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct BootstrapRequest {
    pub username: String,
    pub password: String,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct BootstrapStatusResponse {
    pub bootstrap_required: bool,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct BootstrapResponse {
    pub username: String,
}

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_bootstrap_status))
        .routes(routes!(handle_admin_bootstrap))
}

/// `GET /v1/admin/auth/bootstrap/status`. Public endpoint: returns
/// `{ bootstrap_required: true }` only when the operator collection is
/// empty AND the bootstrap-completed sentinel is unset. Anything else
/// (operators exist, sentinel set, store error) yields `false` — the
/// SPA's response is to show the login page.
#[utoipa::path(
    get,
    path = "/v1/admin/auth/bootstrap/status",
    tag = "bootstrap",
    responses(
        (status = 200, description = "True only when the operator collection is empty and the bootstrap-completed sentinel is unset.", body = BootstrapStatusResponse),
    ),
)]
pub(crate) async fn handle_admin_bootstrap_status(
    State(state): State<Arc<AdminRuntimeState>>,
) -> Json<BootstrapStatusResponse> {
    let sentinel_set = crate::users::bootstrap_completed(&state.users_file).unwrap_or(true);
    let collection_empty = crate::users::load(&state.users_file)
        .map(|c| c.is_empty())
        .unwrap_or(false);
    Json(BootstrapStatusResponse {
        bootstrap_required: !sentinel_set && collection_empty,
    })
}

/// `POST /v1/admin/auth/bootstrap`. Public endpoint: creates the very
/// first admin operator, marks the persistent sentinel, and issues a
/// session in one round trip. After this succeeds, the endpoint
/// refuses further calls even if every operator is later deleted.
#[utoipa::path(
    post,
    path = "/v1/admin/auth/bootstrap",
    tag = "bootstrap",
    request_body = BootstrapRequest,
    responses(
        (status = 200, description = "First operator created; session issued in the same response.", body = BootstrapResponse),
        (status = 400, description = "Validation failed (e.g. password too short).", body = ServiceErrorResponse),
        (status = 409, description = "Bootstrap already completed; further attempts are refused.", body = ServiceErrorResponse),
        (status = 500, description = "Backend could not write the operator collection.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_bootstrap(
    mut auth_session: AdminAuthSession,
    State(state): State<Arc<AdminRuntimeState>>,
    Body(body): Body<BootstrapRequest>,
) -> Result<Json<BootstrapResponse>, AdminError> {
    let username_norm = crate::users::normalise_username(&body.username);
    if username_norm.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_credentials",
            "username is required",
        ));
    }

    // Serialise concurrent bootstrap attempts: two callers must not
    // both see "users empty + sentinel unset" before either has
    // written. The lock is held only across the synchronous DB work
    // — it is released before we hit any `.await` so the request
    // future stays `Send`.
    let bootstrap_guard = state.bootstrap_lock.lock().unwrap();

    match crate::users::bootstrap_completed(&state.users_file) {
        Ok(true) => {
            log_bootstrap_attempt(
                &username_norm,
                Some("bootstrap-already-completed"),
                "failure",
            );
            return Err(AdminError::conflict(
                "bootstrap_already_completed",
                "bootstrap has already been completed for this backend",
            ));
        }
        Ok(false) => {}
        Err(e) => {
            warn!(err = %e, "bootstrap: reading sentinel failed");
            return Err(AdminError::internal_with_code(
                "internal",
                "bootstrap sentinel unreadable",
            ));
        }
    }

    let collection = crate::users::load(&state.users_file).map_err(|e| {
        warn!(err = %e, "bootstrap: loading user collection failed");
        AdminError::internal_with_code("internal", "user collection unavailable")
    })?;
    if !collection.is_empty() {
        log_bootstrap_attempt(&username_norm, Some("users-already-exist"), "failure");
        return Err(AdminError::conflict(
            "users_already_exist",
            "a user already exists; bootstrap requires an empty user collection",
        ));
    }

    if let Err(code) = crate::auth::validate_password(&body.password) {
        log_bootstrap_attempt(&username_norm, Some(code), "failure");
        return Err(AdminError::bad_request(
            code,
            match code {
                "password_empty" => "password is required",
                "password_too_short" => "password too short",
                _ => "password rejected",
            },
        ));
    }

    let now = crate::users::now_epoch_secs();
    let password_hash = crate::auth::hash_password(&body.password).map_err(|e| {
        warn!(err = %e, "bootstrap: argon2 hash failed");
        AdminError::internal_with_code("internal", "could not hash password")
    })?;
    let record = crate::users::UserRecord {
        id: crate::users::new_user_id(),
        username: username_norm.clone(),
        password_hash,
        created_at: now,
        password_changed_at: now,
    };
    let mut new_collection = crate::users::UserCollection::default();
    if let Err(e) = new_collection.add(record.clone()) {
        warn!(err = %e, "bootstrap: building collection failed");
        return Err(AdminError::bad_request(
            "invalid_credentials",
            e.to_string(),
        ));
    }
    if let Err(e) = crate::users::save(&state.users_file, &new_collection) {
        warn!(err = %e, "bootstrap: writing user collection failed");
        return Err(AdminError::internal_with_code(
            "internal",
            "could not persist user",
        ));
    }
    if let Err(e) = crate::users::mark_bootstrap_completed(
        &state.users_file,
        &crate::users::BootstrapSentinel {
            completed_at: now,
            by_username: record.username.clone(),
        },
    ) {
        warn!(err = %e, "bootstrap: writing sentinel failed");
        return Err(AdminError::internal_with_code(
            "internal",
            "could not persist bootstrap sentinel",
        ));
    }
    log_bootstrap_attempt(&record.username, None, "success");
    drop(bootstrap_guard);

    // Issue a session for the brand-new admin: `AuthSession::login`
    // calls `cycle_id()` server-side so the bootstrap response is the
    // first time the SPA learns a session id.
    if let Err(e) = auth_session.login(&record).await {
        warn!(err = ?e, "bootstrap: AuthSession::login failed");
        return Err(AdminError::internal_with_code(
            "internal",
            "failed to issue session",
        ));
    }
    info!(username = %record.username, "bootstrap: first user created");
    Ok(Json(BootstrapResponse {
        username: record.username.clone(),
    }))
}

#[cfg(test)]
mod tests;
