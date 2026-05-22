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

use axum::Router;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};
use tracing::{info, warn};

use crate::admin_api::audit::log_admin_event;
use crate::admin_api::auth::AdminAuthSession;
use crate::http::responses::{Body, error_response, json_response};
use crate::runtime_state::AdminRuntimeState;

#[derive(serde::Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct BootstrapRequest {
    pub username: String,
    pub password: String,
}

#[derive(serde::Serialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct BootstrapStatusResponse {
    pub bootstrap_required: bool,
}

#[derive(serde::Serialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct BootstrapResponse {
    pub username: String,
}

/// Handle `GET /v1/admin/auth/bootstrap/status`. Public endpoint:
/// returns `{ bootstrap_required: true }` only when the operator
/// collection is empty AND the bootstrap-completed sentinel is unset.
/// Anything else (operators exist, sentinel set, store error) yields
/// `false` — the SPA's response is to show the login page.
pub(crate) fn handle_admin_bootstrap_status(state: &AdminRuntimeState) -> Response<Full<Bytes>> {
    let sentinel_set = crate::users::bootstrap_completed(&state.users_file).unwrap_or(true);
    let collection_empty = crate::users::load(&state.users_file)
        .map(|c| c.is_empty())
        .unwrap_or(false);
    json_response(
        StatusCode::OK,
        &BootstrapStatusResponse {
            bootstrap_required: !sentinel_set && collection_empty,
        },
    )
}

/// Handle `POST /v1/admin/auth/bootstrap`. Public endpoint: creates
/// the very first admin operator, marks the persistent sentinel, and
/// issues a session in one round trip. After this succeeds, the
/// endpoint refuses further calls even if every operator is later
/// deleted.
pub(crate) async fn handle_admin_bootstrap(
    mut auth_session: AdminAuthSession,
    state: &AdminRuntimeState,
    body: &BootstrapRequest,
) -> Response<Full<Bytes>> {
    let username_norm = crate::users::normalise_username(&body.username);
    if username_norm.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_credentials",
            "username is required",
        );
    }
    if body.password.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_credentials",
            "password is required",
        );
    }

    // Serialise concurrent bootstrap attempts: two callers must not
    // both see "users empty + sentinel unset" before either has
    // written. The lock is held only across the synchronous DB work
    // — it is released before we hit any `.await` so the request
    // future stays `Send`.
    let bootstrap_guard = state.bootstrap_lock.lock().unwrap();

    match crate::users::bootstrap_completed(&state.users_file) {
        Ok(true) => {
            audit_bootstrap_attempt(
                &username_norm,
                Some("bootstrap-already-completed"),
                "failure",
            );
            return error_response(
                StatusCode::CONFLICT,
                "bootstrap_already_completed",
                "bootstrap has already been completed for this backend",
            );
        }
        Ok(false) => {}
        Err(e) => {
            warn!(err = %e, "bootstrap: reading sentinel failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "bootstrap sentinel unreadable",
            );
        }
    }

    let collection = match crate::users::load(&state.users_file) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "bootstrap: loading user collection failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "user collection unavailable",
            );
        }
    };
    if !collection.is_empty() {
        audit_bootstrap_attempt(&username_norm, Some("users-already-exist"), "failure");
        return error_response(
            StatusCode::CONFLICT,
            "users_already_exist",
            "a user already exists; bootstrap requires an empty user collection",
        );
    }

    let now = crate::users::now_epoch_secs();
    let password_hash = match crate::auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => {
            warn!(err = %e, "bootstrap: argon2 hash failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not hash password",
            );
        }
    };
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
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_credentials",
            &e.to_string(),
        );
    }
    if let Err(e) = crate::users::save(&state.users_file, &new_collection) {
        warn!(err = %e, "bootstrap: writing user collection failed");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "could not persist user",
        );
    }
    if let Err(e) = crate::users::mark_bootstrap_completed(
        &state.users_file,
        &crate::users::BootstrapSentinel {
            completed_at: now,
            by_username: record.username.clone(),
        },
    ) {
        warn!(err = %e, "bootstrap: writing sentinel failed");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "could not persist bootstrap sentinel",
        );
    }
    audit_bootstrap_attempt(&record.username, None, "success");
    drop(bootstrap_guard);

    // Issue a session for the brand-new admin: `AuthSession::login`
    // calls `cycle_id()` server-side so the bootstrap response is the
    // first time the SPA learns a session id.
    if let Err(e) = auth_session.login(&record).await {
        warn!(err = ?e, "bootstrap: AuthSession::login failed");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "failed to issue session",
        );
    }
    info!(username = %record.username, "bootstrap: first user created");
    json_response(
        StatusCode::OK,
        &BootstrapResponse {
            username: record.username.clone(),
        },
    )
}

pub(crate) fn public_router() -> Router<Arc<AdminRuntimeState>> {
    Router::new()
        .route(
            "/v1/admin/auth/bootstrap/status",
            get(axum_bootstrap_status),
        )
        .route("/v1/admin/auth/bootstrap", post(axum_bootstrap))
}

async fn axum_bootstrap_status(
    State(state): State<Arc<AdminRuntimeState>>,
) -> Response<Full<Bytes>> {
    handle_admin_bootstrap_status(&state)
}

async fn axum_bootstrap(
    auth_session: AdminAuthSession,
    State(state): State<Arc<AdminRuntimeState>>,
    Body(parsed): Body<BootstrapRequest>,
) -> axum::response::Response {
    handle_admin_bootstrap(auth_session, &state, &parsed)
        .await
        .into_response()
}

/// Log one bootstrap attempt outcome. Best effort: never fails the caller.
pub(crate) fn audit_bootstrap_attempt(username: &str, operation: Option<&str>, result: &str) {
    log_admin_event(
        "bootstrap.attempt",
        username,
        None,
        None,
        None,
        operation,
        result,
    );
}

#[cfg(test)]
mod tests;
