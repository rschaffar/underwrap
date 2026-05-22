//! Admin login surface: `/v1/admin/auth/{login, logout, whoami, password}`.
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

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum_login::AuthSession;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};
use tracing::{info, warn};

use crate::auth::{AuthBackend, AuthError, Credentials};
use crate::http::responses::{Body, error_response, json_response};
use crate::runtime_state::AdminRuntimeState;
use crate::users::{Principal, UserRecord};

pub(crate) type AdminAuthSession = AuthSession<AuthBackend>;

#[derive(serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct LoginResponse {
    pub username: String,
}

#[derive(serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct WhoamiResponse {
    pub authenticated: bool,
    // `skip_serializing_if = "Option::is_none"` omits the JSON key when None;
    // `#[ts(optional)]` makes ts-rs emit `username?: string` (instead of the
    // default `string | null`) so the SPA type matches the wire format.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
}

pub(crate) async fn handle_admin_login(
    mut auth_session: AdminAuthSession,
    body: LoginRequest,
) -> Response<Full<Bytes>> {
    let username = body.username.clone();
    let creds = Credentials {
        username: body.username,
        password: body.password,
    };
    let user = match auth_session.authenticate(creds).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            info!(username = %username, "login failed");
            return error_response(
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "invalid username or password",
            );
        }
        Err(axum_login::Error::Backend(AuthError::RateLimited)) => {
            warn!(username = %username, "login rate-limited");
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "too many login attempts; please wait and try again",
            );
        }
        Err(axum_login::Error::Backend(AuthError::UserCollection(err))) => {
            warn!(err = %err, "login: failed to read user collection");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "user collection unavailable",
            );
        }
        Err(axum_login::Error::Session(err)) => {
            warn!(err = ?err, "login: session backend failure");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "session backend unavailable",
            );
        }
    };
    if let Err(e) = auth_session.login(&user).await {
        warn!(err = ?e, "login: AuthSession::login failed");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "failed to issue session",
        );
    }
    info!(username = %user.username, "login succeeded");
    json_response(
        StatusCode::OK,
        &LoginResponse {
            username: user.username,
        },
    )
}

pub(crate) async fn handle_admin_logout(
    mut auth_session: AdminAuthSession,
) -> Response<Full<Bytes>> {
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
    json_response(StatusCode::OK, &serde_json::json!({}))
}

pub(crate) async fn handle_admin_whoami(auth_session: AdminAuthSession) -> Response<Full<Bytes>> {
    // whoami is intentionally tolerant of missing auth (returns
    // `authenticated: false`) so the SPA can use it to drive the
    // login / bootstrap redirect dance without producing 401s in the
    // browser devtools. `axum_login` already invalidated the session
    // if the operator was deleted or had its password rotated since
    // it was issued, so `auth_session.user.is_some()` is the
    // authoritative answer here.
    match auth_session.user {
        Some(u) => json_response(
            StatusCode::OK,
            &WhoamiResponse {
                authenticated: true,
                username: Some(u.username),
            },
        ),
        None => json_response(
            StatusCode::OK,
            &WhoamiResponse {
                authenticated: false,
                username: None,
            },
        ),
    }
}

/// `POST /v1/admin/auth/password` — self-service password change for
/// the currently authenticated user. Requires the current password as
/// proof-of-possession.
pub(crate) async fn handle_admin_password_change(
    mut auth_session: AdminAuthSession,
    state: Arc<AdminRuntimeState>,
    principal: Principal,
    body: underwrap_protocol::service::AdminPasswordChangeRequest,
) -> Response<Full<Bytes>> {
    use crate::admin_api::users::{mutate_collection, write_admin_user_audit};
    use underwrap_protocol::service::AdminOkUserResponse;

    if body.new_password.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_password",
            "new password must not be empty",
        );
    }
    let collection = match crate::users::load(&state.users_file) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "auth.password: collection unreadable");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "user collection unavailable",
            );
        }
    };
    let current_hash = match collection.find_by_id(&principal.id) {
        Some(u) => u.password_hash.clone(),
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "session refers to a user that no longer exists",
            );
        }
    };
    if !crate::auth::verify_password(&current_hash, &body.current_password) {
        write_admin_user_audit(
            &principal,
            &principal.username,
            "user.password.change",
            Some("wrong-current-password"),
            "failure",
        );
        return error_response(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "current password does not match",
        );
    }
    let new_hash = match crate::auth::hash_password(&body.new_password) {
        Ok(h) => h,
        Err(e) => {
            warn!(err = %e, "auth.password: argon2 hash failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not hash password",
            );
        }
    };
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
            write_admin_user_audit(
                &principal,
                &principal.username,
                "user.password.change",
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
            json_response(StatusCode::OK, &AdminOkUserResponse { ok: true })
        }
        Err(e) => {
            warn!(err = %e, "auth.password: save failed");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not save new password",
            )
        }
    }
}

pub(crate) fn public_router() -> Router<Arc<AdminRuntimeState>> {
    Router::new()
        .route("/v1/admin/auth/login", post(axum_login_route))
        .route("/v1/admin/auth/whoami", get(axum_whoami))
}

pub(crate) fn protected_router() -> Router<Arc<AdminRuntimeState>> {
    Router::new()
        .route("/v1/admin/auth/password", post(axum_password_change))
        .route("/v1/admin/auth/logout", post(axum_logout))
}

async fn axum_login_route(
    auth_session: AdminAuthSession,
    Body(parsed): Body<LoginRequest>,
) -> axum::response::Response {
    handle_admin_login(auth_session, parsed)
        .await
        .into_response()
}

async fn axum_whoami(auth_session: AdminAuthSession) -> axum::response::Response {
    handle_admin_whoami(auth_session).await.into_response()
}

async fn axum_logout(auth_session: AdminAuthSession) -> axum::response::Response {
    handle_admin_logout(auth_session).await.into_response()
}

async fn axum_password_change(
    auth_session: AdminAuthSession,
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(parsed): Body<underwrap_protocol::service::AdminPasswordChangeRequest>,
) -> axum::response::Response {
    handle_admin_password_change(auth_session, state, principal, parsed)
        .await
        .into_response()
}

#[cfg(test)]
mod tests;
