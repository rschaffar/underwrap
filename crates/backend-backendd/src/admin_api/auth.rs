//! Admin login surface: `/v1/admin/auth/{login, logout, whoami, password}`.
//!
//! Login issues a session cookie + matching CSRF cookie. Logout
//! revokes the current session id server-side and clears both
//! cookies. Whoami is intentionally tolerant of missing auth so the
//! SPA can use it for the login / bootstrap redirect dance. Password
//! change is the self-service "rotate my own password" surface.

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use tracing::{info, warn};

use crate::http::responses::{error_response, json_response, parse_json_body};
use crate::runtime_state::AdminRuntimeState;

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
    state: &AdminRuntimeState,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let session_key = state.session_key;
    let body = match parse_json_body::<LoginRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };

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

    if !state.login_rate_limiter.check_and_record(&username_norm) {
        warn!(username = %username_norm, "login rate-limited");
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many login attempts; please wait and try again",
        );
    }

    let collection = match crate::users::load(&state.users_file) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "login: failed to read user collection");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "user collection unavailable",
            );
        }
    };

    // Always run argon2 verify, even on unknown user, to keep timing
    // independent of user existence.
    let lookup = collection.find_by_username(&username_norm);
    let hash_to_check = lookup
        .map(|u| u.password_hash.as_str())
        .unwrap_or(crate::auth::DUMMY_PASSWORD_HASH);
    let password_ok = crate::auth::verify_password(hash_to_check, &body.password);

    let user = match (lookup, password_ok) {
        (Some(u), true) => u,
        _ => {
            info!(username = %username_norm, "login failed");
            return error_response(
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "invalid username or password",
            );
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let payload = crate::auth::new_session_payload(
        user.id.clone(),
        user.username.clone(),
        now,
        state.session_lifetime_secs,
    );
    let encoded = match crate::auth::encrypt_session(&payload, &session_key) {
        Ok(s) => s,
        Err(e) => {
            warn!(err = %e, "login: failed to encrypt session");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "failed to issue session",
            );
        }
    };
    let session_cookie = crate::auth::render_session_cookie(
        &encoded,
        state.session_lifetime_secs,
        state.cookies_secure,
    );
    let csrf_cookie = crate::auth::render_csrf_cookie(
        &payload.csrf_secret,
        state.session_lifetime_secs,
        state.cookies_secure,
    );

    state.login_rate_limiter.reset(&username_norm);
    info!(username = %user.username, "login succeeded");

    let body = LoginResponse {
        username: user.username.clone(),
    };
    let body_bytes = serde_json::to_vec(&body).expect("serializing login response");
    Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .header(hyper::header::SET_COOKIE, session_cookie)
        .header(hyper::header::SET_COOKIE, csrf_cookie)
        .body(Full::new(Bytes::from(body_bytes)))
        .expect("building login response")
}

/// Handle `POST /v1/admin/auth/logout`. Revokes the current session id
/// server-side and clears both cookies in the response. The CSRF
/// double-submit check is enforced by `handle_admin_request` before we
/// get here, so the caller has already proved possession of the
/// session's csrf_secret.
pub(crate) fn handle_admin_logout(
    state: &AdminRuntimeState,
    session: &crate::auth::SessionPayload,
) -> Response<Full<Bytes>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    state
        .session_revocations
        .revoke(session.session_id.clone(), session.expires_at, now);
    info!(username = %session.username, "logout: session revoked");
    Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .header(
            hyper::header::SET_COOKIE,
            crate::auth::render_session_cookie_clear(state.cookies_secure),
        )
        .header(
            hyper::header::SET_COOKIE,
            crate::auth::render_csrf_cookie_clear(state.cookies_secure),
        )
        .body(Full::new(Bytes::from_static(b"{}")))
        .expect("building logout response")
}

pub(crate) fn handle_admin_whoami(
    state: &AdminRuntimeState,
    headers: &hyper::HeaderMap,
) -> Response<Full<Bytes>> {
    // whoami is intentionally tolerant of missing auth (returns
    // authenticated:false) so the SPA can use it to drive the
    // login/bootstrap redirect dance without producing 401s in the
    // browser devtools.
    let Some(value) = crate::auth::extract_session_cookie(headers) else {
        return json_response(
            StatusCode::OK,
            &WhoamiResponse {
                authenticated: false,
                username: None,
            },
        );
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let payload = match crate::auth::decrypt_session(value, &state.session_key, now) {
        Ok(p) => p,
        Err(_) => {
            return json_response(
                StatusCode::OK,
                &WhoamiResponse {
                    authenticated: false,
                    username: None,
                },
            );
        }
    };
    // Confirm the username still resolves to a live user record; a
    // deleted user invalidates the session.
    let collection = crate::users::load(&state.users_file).unwrap_or_default();
    let exists = collection.find_by_username(&payload.username).is_some();
    json_response(
        StatusCode::OK,
        &WhoamiResponse {
            authenticated: exists,
            username: Some(payload.username),
        },
    )
}

/// `POST /v1/admin/auth/password` — self-service password change for
/// the currently authenticated user. Requires the current password as
/// proof-of-possession.
#[allow(clippy::result_large_err)]
pub(crate) async fn handle_admin_password_change(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    use crate::admin_api::users::{mutate_collection, write_admin_user_audit};
    use underwrap_protocol::service::{AdminOkUserResponse, AdminPasswordChangeRequest};

    let body = match parse_json_body::<AdminPasswordChangeRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
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
            state,
            principal,
            &principal.username,
            "user.password.change",
            "wrong_current_password",
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
    let now = crate::users::now_epoch_secs();
    let result = mutate_collection(state, |c| {
        if c.find_by_id(&id_owned).is_none() {
            anyhow::bail!("not_found");
        }
        c.update(&id_owned, |u| {
            u.password_hash = new_hash.clone();
            u.password_changed_at = now;
        })?;
        Ok(())
    });
    match result {
        Ok(()) => {
            write_admin_user_audit(
                state,
                principal,
                &principal.username,
                "user.password.change",
                "success",
            );
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

/// Authenticated auth routes — currently just `auth/password`.
/// Login + whoami are public (dispatched inline before the
/// session-cookie gate). `auth/logout` is dispatched inline in
/// [`crate::admin_api::handle_admin_request`] because it consumes the
/// `AdminAuth` struct, not the resolved `Principal`.
pub(crate) async fn route(
    method: &Method,
    path: &str,
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    match (method, path) {
        (&Method::POST, "/v1/admin/auth/password") => {
            handle_admin_password_change(state, principal, request).await
        }
        _ => error_response(StatusCode::NOT_FOUND, "not_found", "unknown admin endpoint"),
    }
}

#[cfg(test)]
mod tests;
