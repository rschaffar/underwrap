//! Admin API authentication and CSRF infrastructure.
//!
//! Sessions are managed by [`tower_sessions`] (in-memory store, opaque
//! ID cookie) and the per-request user lookup is provided by
//! [`axum_login::AuthSession`] over [`crate::auth::AuthBackend`]. This
//! module owns:
//!
//! - [`build_csrf_config`] — the project-wide [`axum_csrf::CsrfConfig`]
//!   (cookie name, attributes, HMAC salt).
//! - [`require_login_layer`] — a small predicate middleware that
//!   short-circuits with our canonical `{code, message}` 401 envelope
//!   when no user is on the [`AuthSession`]. Equivalent in behaviour to
//!   `axum_login::login_required!`, but emits the same JSON error shape
//!   as the rest of the admin API.
//! - [`verify_csrf_layer`] — verifies `X-CSRF-Token` against the
//!   request's [`CsrfToken`] via HMAC on state-changing methods.
//! - [`attach_csrf_response_layer`] — writes the CSRF `Set-Cookie` and
//!   the matching `X-CSRF-Token` response header on every response so
//!   the SPA can pick the token off the response and echo it on the
//!   next state-changing call.
//! - [`resolve_principal_layer`] — turns the authenticated
//!   [`UserRecord`] into a live [`Principal`] (re-read from the
//!   collection so a rename takes effect immediately) and inserts it
//!   as a request extension.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response as AxumResponse};
use axum_csrf::{CsrfConfig, CsrfToken};
use axum_login::AuthSession;
use hyper::header::HeaderValue;
use hyper::{Method, StatusCode};
use tower_sessions::cookie::SameSite;
use tracing::{info, warn};

use crate::auth::AuthBackend;
use crate::http::responses::error_response;
use crate::runtime_state::AdminRuntimeState;
use crate::users::{Principal, UserRecord};

/// CSRF cookie name. The `__Host-` prefix requires `Secure; Path=/`
/// and forbids a `Domain` attribute, which closes off cookie-write
/// attacks from related subdomains.
pub(crate) const CSRF_COOKIE_NAME: &str = "__Host-uw-csrf";

/// HTTP header the SPA sends carrying the HMAC-authenticity token for
/// the request's CSRF cookie.
pub(crate) const CSRF_HEADER_NAME: &str = "X-CSRF-Token";

/// HMAC salt used by `axum_csrf` to derive the per-request
/// authenticity token from the cookie's random token. A fresh value
/// per daemon restart is fine — sessions don't survive restarts
/// either (in-memory store), and there is no requirement for tokens
/// to remain valid across restarts.
fn random_csrf_salt() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Build the project-wide CSRF config. `cookies_secure` should be true
/// whenever the admin listener terminates TLS.
pub(crate) fn build_csrf_config(cookies_secure: bool) -> CsrfConfig {
    CsrfConfig::default()
        .with_cookie_name(CSRF_COOKIE_NAME)
        .with_cookie_path("/")
        .with_cookie_same_site(SameSite::Strict)
        .with_secure(cookies_secure)
        .with_http_only(true)
        .with_salt(random_csrf_salt())
}

/// Reject the request with our canonical 401 JSON envelope when no
/// user is attached to the session.
pub(crate) async fn require_login_layer(
    auth_session: AuthSession<AuthBackend>,
    request: Request,
    next: Next,
) -> AxumResponse {
    if auth_session.user.is_some() {
        next.run(request).await
    } else {
        warn!("rejected admin request: not authenticated");
        error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "authentication required",
        )
        .into_response()
    }
}

/// Verify `X-CSRF-Token` against the request's [`CsrfToken`] on
/// state-changing methods. `GET`/`HEAD`/`OPTIONS` are exempt.
pub(crate) async fn verify_csrf_layer(request: Request, next: Next) -> AxumResponse {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return next.run(request).await;
    }
    let Some(token) = request.extensions().get::<CsrfToken>().cloned() else {
        warn!("rejected admin request: CSRF layer missing");
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "CSRF layer not initialised",
        )
        .into_response();
    };
    let header_value = request
        .headers()
        .get(CSRF_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(header_value) = header_value else {
        warn!("rejected admin request: missing X-CSRF-Token");
        return error_response(
            StatusCode::FORBIDDEN,
            "csrf_required",
            "missing or empty X-CSRF-Token on state-changing request",
        )
        .into_response();
    };
    if token.verify(header_value).is_err() {
        warn!("rejected admin request: X-CSRF-Token did not verify");
        return error_response(
            StatusCode::FORBIDDEN,
            "csrf_mismatch",
            "X-CSRF-Token does not match session",
        )
        .into_response();
    }
    next.run(request).await
}

/// Append the CSRF `Set-Cookie` and the matching `X-CSRF-Token`
/// response header on every response. This lets the SPA learn the
/// current authenticity token off the most recent response without
/// having to call a dedicated endpoint.
pub(crate) async fn attach_csrf_response_layer(request: Request, next: Next) -> AxumResponse {
    let token = request.extensions().get::<CsrfToken>().cloned();
    let response = next.run(request).await;
    let Some(token) = token else {
        return response;
    };
    let header_value = token.authenticity_token().ok();
    // `(CsrfToken, axum_response::Response).into_response()` applies
    // the token's `IntoResponseParts` impl — that is the path through
    // `axum_csrf` that emits the `Set-Cookie` for the CSRF cookie.
    let mut response = (token, response).into_response();
    if let Some(value) = header_value
        && let Ok(v) = HeaderValue::from_str(&value)
    {
        response.headers_mut().insert(CSRF_HEADER_NAME, v);
    }
    response
}

/// Read the authenticated [`UserRecord`] from the request's
/// [`AuthSession`], re-resolve the live operator (in case of rename),
/// log a structured admin event, and insert a [`Principal`] extension
/// for downstream handlers.
pub(crate) async fn resolve_principal_layer(
    State(state): State<Arc<AdminRuntimeState>>,
    auth_session: AuthSession<AuthBackend>,
    mut request: Request,
    next: Next,
) -> AxumResponse {
    let Some(user) = auth_session.user.clone() else {
        // `require_login_layer` runs ahead of us, so this is a bug:
        // the chain didn't gate authentication. Fail closed.
        return error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "authentication required",
        )
        .into_response();
    };
    let principal = match resolve_live_principal(&state, &user) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    info!(
        user = %principal.username,
        method = %request.method(),
        path = request.uri().path(),
        "admin request",
    );
    request.extensions_mut().insert(principal);
    next.run(request).await
}

/// Look up the operator record from disk so a rename or password
/// change since the session was issued is reflected on this request.
/// A missing record (operator deleted between requests) maps to a 401
/// — the session's stored auth hash will mismatch on the next
/// request anyway, but rejecting here keeps the response consistent.
#[allow(clippy::result_large_err)]
fn resolve_live_principal(
    state: &AdminRuntimeState,
    user: &UserRecord,
) -> std::result::Result<Principal, AxumResponse> {
    let collection = match crate::users::load(&state.users_file) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "could not load user collection");
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "user collection unavailable",
            )
            .into_response());
        }
    };
    match collection.find_by_id(&user.id) {
        Some(live) => Ok(Principal {
            id: live.id.clone(),
            username: live.username.clone(),
        }),
        None => Err(error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "session refers to a user that no longer exists",
        )
        .into_response()),
    }
}

#[cfg(test)]
mod tests;
