//! Admin login surface: `/v1/admin/auth/{login, logout, whoami,
//! password, bootstrap, bootstrap/status}`.
//!
//! Sessions are issued via [`tower_sessions`] / [`axum_login`]: on a
//! successful password check the handler calls
//! [`AuthSession::login`], which writes an opaque session id to the
//! `__Host-uw-session` cookie and persists the user id + password-hash
//! bytes in the in-memory store. Logout calls
//! [`AuthSession::logout`], which flushes the store entry and clears
//! the cookie. Whoami is tolerant of missing auth so the SPA can use
//! it to drive the login / bootstrap redirect dance without producing
//! 401s.
//!
//! The [`bootstrap`] submodule covers the once-only "first operator"
//! setup that runs against an empty operator collection. Its routes
//! share this module's `/v1/admin/auth/*` prefix and are merged into
//! [`public_router`] below — there is no separate top-level
//! `bootstrap` module.

pub mod bootstrap;

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum_login::AuthSession;
use tracing::{info, warn};
use underwrap_protocol::service::{
    AdminOkUserResponse, AdminPasswordChangeRequest, ServiceErrorResponse,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::auth::{AuthBackend, AuthError, Credentials};
use crate::http::responses::Body;
use crate::runtime_state::AdminRuntimeState;
use crate::users::{Principal, UserRecord};

pub(crate) type AdminAuthSession = AuthSession<AuthBackend>;

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct LoginResponse {
    pub username: String,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct WhoamiResponse {
    /// Present when a session is attached; absent when the request is
    /// unauthenticated. The SPA treats `username !== undefined` as
    /// the authenticated check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct EmptyObject {}

pub(crate) fn public_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_login))
        .routes(routes!(handle_admin_whoami))
        .merge(bootstrap::openapi_router())
}

pub(crate) fn protected_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_password_change))
        .routes(routes!(handle_admin_logout))
}

#[utoipa::path(
    post,
    path = "/v1/admin/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login succeeded; session cookie set.", body = LoginResponse),
        (status = 401, description = "Invalid username or password.", body = ServiceErrorResponse),
        (status = 429, description = "Rate-limited; too many login attempts.", body = ServiceErrorResponse),
        (status = 500, description = "Backend could not read user collection.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_login(
    mut auth_session: AdminAuthSession,
    Body(body): Body<LoginRequest>,
) -> Result<Json<LoginResponse>, AdminError> {
    let username = body.username.clone();
    let creds = Credentials {
        username: body.username,
        password: body.password,
    };
    let user = match auth_session.authenticate(creds).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            info!(username = %username, "login failed");
            return Err(AdminError::unauthorized(
                "invalid_credentials",
                "invalid username or password",
            ));
        }
        Err(axum_login::Error::Backend(AuthError::RateLimited)) => {
            warn!(username = %username, "login rate-limited");
            return Err(AdminError::too_many_requests(
                "rate_limited",
                "too many login attempts; please wait and try again",
            ));
        }
        Err(axum_login::Error::Backend(AuthError::UserCollection(err))) => {
            warn!(err = %err, "login: failed to read user collection");
            return Err(AdminError::internal_with_code(
                "internal",
                "user collection unavailable",
            ));
        }
        Err(axum_login::Error::Session(err)) => {
            warn!(err = ?err, "login: session backend failure");
            return Err(AdminError::internal_with_code(
                "internal",
                "session backend unavailable",
            ));
        }
    };
    if let Err(e) = auth_session.login(&user).await {
        warn!(err = ?e, "login: AuthSession::login failed");
        return Err(AdminError::internal_with_code(
            "internal",
            "failed to issue session",
        ));
    }
    info!(username = %user.username, "login succeeded");
    Ok(Json(LoginResponse {
        username: user.username,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/auth/logout",
    tag = "auth",
    security(("session_cookie" = []), ("csrf_header" = [])),
    responses(
        (status = 204, description = "Session destroyed and cookie cleared."),
    ),
)]
pub(crate) async fn handle_admin_logout(mut auth_session: AdminAuthSession) -> Json<EmptyObject> {
    let username = auth_session.user.as_ref().map(|u| u.username.clone());
    if let Err(e) = auth_session.logout().await {
        warn!(err = ?e, "logout: AuthSession::logout failed");
        // Fall through — we still want to respond 200 with the
        // session-clearing cookie that tower-sessions emits, so a
        // partially failed flush doesn't strand the client.
    }
    if let Some(name) = username {
        info!(username = %name, "logout: session flushed");
    }
    Json(EmptyObject {})
}

#[utoipa::path(
    get,
    path = "/v1/admin/auth/whoami",
    tag = "auth",
    responses(
        (status = 200, description = "Returns the current operator, or an empty object if no session is present. Never 401 — designed for the SPA's bootstrap-redirect dance.", body = WhoamiResponse),
    ),
)]
pub(crate) async fn handle_admin_whoami(auth_session: AdminAuthSession) -> Json<WhoamiResponse> {
    // whoami is intentionally tolerant of missing auth so the SPA
    // can drive the login / bootstrap redirect dance without
    // producing 401s in the browser devtools. `axum_login` already
    // invalidated the session if the operator was deleted or had its
    // password rotated since it was issued, so `auth_session.user`
    // is the authoritative answer here.
    Json(WhoamiResponse {
        username: auth_session.user.map(|u| u.username),
    })
}

/// `POST /v1/admin/auth/password` — self-service password change for
/// the currently authenticated user. Requires the current password as
/// proof-of-possession.
#[utoipa::path(
    post,
    path = "/v1/admin/auth/password",
    tag = "auth",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminPasswordChangeRequest,
    responses(
        (status = 200, description = "Password changed; new session issued.", body = AdminOkUserResponse),
        (status = 400, description = "Validation failed (e.g. new password too short).", body = ServiceErrorResponse),
        (status = 401, description = "Current password is incorrect.", body = ServiceErrorResponse),
        (status = 500, description = "Backend could not persist the change.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_password_change(
    mut auth_session: AdminAuthSession,
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminPasswordChangeRequest>,
) -> Result<Json<AdminOkUserResponse>, AdminError> {
    use crate::admin_api::audit::{AuditEntity, log_admin_event};
    use crate::admin_api::users::mutate_collection;

    if body.new_password.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_password",
            "new password must not be empty",
        ));
    }
    let collection = crate::users::load(&state.users_file).map_err(|e| {
        warn!(err = %e, "auth.password: collection unreadable");
        AdminError::internal_with_code("internal", "user collection unavailable")
    })?;
    let current_hash = match collection.find_by_id(&principal.id) {
        Some(u) => u.password_hash.clone(),
        None => {
            return Err(AdminError::unauthorized(
                "unauthorized",
                "session refers to a user that no longer exists",
            ));
        }
    };
    if !crate::auth::verify_password(&current_hash, &body.current_password) {
        log_admin_event(
            "user.password.change",
            &principal,
            AuditEntity::User {
                username: &principal.username,
            },
            Some("wrong-current-password"),
            "failure",
        );
        return Err(AdminError::unauthorized(
            "invalid_credentials",
            "current password does not match",
        ));
    }
    let new_hash = crate::auth::hash_password(&body.new_password).map_err(|e| {
        warn!(err = %e, "auth.password: argon2 hash failed");
        AdminError::internal_with_code("internal", "could not hash password")
    })?;
    let id_owned = principal.id.clone();
    let new_hash_for_closure = new_hash.clone();
    let now = crate::users::now_epoch_secs();
    let result = mutate_collection(&state, |c| {
        c.update(&id_owned, |u| {
            u.password_hash = new_hash_for_closure.clone();
            u.password_changed_at = now;
        })?;
        Ok(())
    });
    match result {
        Ok(()) => {
            log_admin_event(
                "user.password.change",
                &principal,
                AuditEntity::User {
                    username: &principal.username,
                },
                None,
                "success",
            );
            // The user's other sessions (and indeed every other
            // session ever issued under the *old* password hash)
            // will be invalidated automatically the next time their
            // owners hit any admin URL — `axum_login` re-reads
            // `session_auth_hash()` on every request and flushes the
            // session when it doesn't match. Refresh *this*
            // session's stored hash so the operator that just
            // changed their own password isn't immediately
            // logged out on the next request.
            let updated_user = UserRecord {
                id: principal.id.clone(),
                username: principal.username.clone(),
                password_hash: new_hash,
                created_at: 0,
                password_changed_at: now,
            };
            if let Err(e) = auth_session.login(&updated_user).await {
                warn!(err = ?e, "auth.password: refreshing AuthSession after password change");
                // Not fatal: response still succeeds, but the
                // operator will need to log in again.
            }
            Ok(Json(AdminOkUserResponse { ok: true }))
        }
        Err(e) => {
            warn!(err = %e, "auth.password: save failed");
            Err(AdminError::internal_with_code(
                "internal",
                "could not save new password",
            ))
        }
    }
}

#[cfg(test)]
mod tests;
