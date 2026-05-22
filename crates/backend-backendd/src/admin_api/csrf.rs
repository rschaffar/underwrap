//! Session-cookie authentication, CSRF double-submit gate, principal
//! resolver, and CSRF-cookie refresher for the admin API.
//!
//! Every admin request that targets a non-public endpoint flows
//! through, in order:
//!
//! 1. [`authenticate_admin`] — decrypts the AEAD session cookie and
//!    rejects revoked sessions.
//! 2. [`check_csrf`] — on state-changing methods, verifies the
//!    `X-CSRF-Token` header matches both the `__Host-uw-csrf` cookie
//!    (double-submit) and the value bound into the session payload
//!    (so cookie substitution alone doesn't pass).
//! 3. [`resolve_principal`] — re-reads the user collection so renames
//!    take effect immediately.
//! 4. After the handler runs, [`attach_csrf_cookie`] refreshes the
//!    `Set-Cookie` so the SPA always has a value matching the live
//!    session.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::IntoResponse;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Method, Response, StatusCode};
use tracing::{info, warn};

use crate::http::responses::error_response;
use crate::runtime_state::AdminRuntimeState;

/// Result of authenticating an admin request.
#[derive(Clone, Debug)]
pub(crate) struct AdminAuth(pub(crate) crate::auth::SessionPayload);

impl AdminAuth {
    #[cfg(test)]
    pub(crate) fn operator(&self) -> &str {
        &self.0.username
    }

    pub(crate) fn session(&self) -> &crate::auth::SessionPayload {
        &self.0
    }
}

/// Reasons `authenticate_admin` can fail. Mapped to HTTP responses by
/// the caller — `Revoked` becomes 401 with `session_revoked`, the rest
/// become 401 with `unauthorized`.
#[derive(Debug)]
pub(crate) enum AdminAuthError {
    Unauthenticated(&'static str),
    Revoked,
}

/// Authenticate an admin request via its session cookie.
pub(crate) fn authenticate_admin(
    state: &AdminRuntimeState,
    headers: &hyper::HeaderMap,
) -> std::result::Result<AdminAuth, AdminAuthError> {
    let value = crate::auth::extract_session_cookie(headers)
        .ok_or(AdminAuthError::Unauthenticated("missing session cookie"))?;
    let now = crate::users::now_epoch_secs();
    match crate::auth::decrypt_session(value, &state.session_key, now) {
        Ok(payload) => {
            if state
                .session_revocations
                .is_revoked(&payload.session_id, now)
                || state
                    .session_revocations
                    .is_user_floor_revoked(&payload.username, payload.issued_at)
            {
                Err(AdminAuthError::Revoked)
            } else {
                Ok(AdminAuth(payload))
            }
        }
        Err(_) => Err(AdminAuthError::Unauthenticated("invalid session cookie")),
    }
}

/// Verify the CSRF double-submit on a state-changing session-authenticated
/// request. Returns `Ok(())` when the request is safe to dispatch.
/// `GET/HEAD/OPTIONS` are exempt.
pub(crate) fn check_csrf(
    method: &Method,
    headers: &hyper::HeaderMap,
    session_csrf_secret: &str,
) -> std::result::Result<(), (&'static str, &'static str)> {
    if matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS) {
        return Ok(());
    }
    let header_value = headers
        .get(crate::auth::CSRF_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let header_value = match header_value {
        Some(v) => v,
        None => {
            return Err((
                "csrf_required",
                "missing or empty X-CSRF-Token on state-changing request",
            ));
        }
    };
    let cookie_value = match crate::auth::extract_csrf_cookie(headers) {
        Some(v) if !v.is_empty() => v,
        _ => return Err(("csrf_required", "missing CSRF cookie")),
    };
    use subtle::ConstantTimeEq;
    // Header must match the cookie (double-submit) AND match the value
    // bound into the session (so substituted cookies don't pass).
    let header_bytes = header_value.as_bytes();
    let cookie_bytes = cookie_value.as_bytes();
    let secret_bytes = session_csrf_secret.as_bytes();
    if header_bytes.len() != cookie_bytes.len() || !bool::from(header_bytes.ct_eq(cookie_bytes)) {
        return Err(("csrf_mismatch", "X-CSRF-Token does not match CSRF cookie"));
    }
    if header_bytes.len() != secret_bytes.len() || !bool::from(header_bytes.ct_eq(secret_bytes)) {
        return Err(("csrf_mismatch", "X-CSRF-Token does not match session"));
    }
    Ok(())
}

/// Build a [`crate::users::Principal`] from an authenticated request.
/// We re-read the user collection so the current username (after a
/// rename) is always reflected.
#[allow(clippy::result_large_err)]
pub(crate) fn resolve_principal(
    state: &AdminRuntimeState,
    auth: &AdminAuth,
) -> std::result::Result<crate::users::Principal, Response<Full<Bytes>>> {
    let payload = auth.session();
    let collection = match crate::users::load(&state.users_file) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "could not load user collection");
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "user collection unavailable",
            ));
        }
    };
    match collection.find_by_username(&payload.username) {
        Some(u) => Ok(crate::users::Principal {
            id: u.id.clone(),
            username: u.username.clone(),
        }),
        None => Err(error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "session refers to a user that no longer exists",
        )),
    }
}

/// Refresh the CSRF cookie on every authenticated admin response so
/// the SPA always has a value matching the live session.
///
/// The production path uses [`attach_csrf_cookie_layer`], which inlines
/// the same logic into an Axum middleware. This standalone form is
/// retained as a unit-test entry point.
#[cfg(test)]
pub(crate) fn attach_csrf_cookie(
    mut response: Response<Full<Bytes>>,
    state: &AdminRuntimeState,
    auth: &AdminAuth,
) -> Response<Full<Bytes>> {
    let payload = auth.session();
    let cookie = crate::auth::render_csrf_cookie(
        &payload.csrf_secret,
        state.session_lifetime_secs,
        state.cookies_secure,
    );
    match hyper::header::HeaderValue::from_str(&cookie) {
        Ok(v) => {
            response.headers_mut().append(hyper::header::SET_COOKIE, v);
        }
        Err(e) => {
            warn!(err = %e, "could not append CSRF cookie header");
        }
    }
    response
}

/// Axum middleware that authenticates the request via the session
/// cookie and inserts the `AdminAuth` extension. Short-circuits with
/// 401 on missing/invalid/revoked.
pub(crate) async fn authenticate_layer(
    State(state): State<Arc<AdminRuntimeState>>,
    mut request: Request,
    next: Next,
) -> axum::response::Response {
    let auth = match authenticate_admin(&state, request.headers()) {
        Ok(a) => a,
        Err(AdminAuthError::Revoked) => {
            warn!("rejected admin request: session revoked");
            return error_response(
                StatusCode::UNAUTHORIZED,
                "session_revoked",
                "session has been revoked",
            )
            .into_response();
        }
        Err(AdminAuthError::Unauthenticated(reason)) => {
            warn!(reason = reason, "rejected admin request");
            return error_response(StatusCode::UNAUTHORIZED, "unauthorized", reason)
                .into_response();
        }
    };
    request.extensions_mut().insert(auth);
    next.run(request).await
}

/// Axum middleware that runs the CSRF double-submit check against the
/// `AdminAuth` extension. Must run after [`authenticate_layer`].
pub(crate) async fn check_csrf_layer(request: Request, next: Next) -> axum::response::Response {
    let auth = request
        .extensions()
        .get::<AdminAuth>()
        .expect("authenticate_layer must run before check_csrf_layer")
        .clone();
    if let Err((code, msg)) = check_csrf(request.method(), request.headers(), &auth.0.csrf_secret) {
        warn!(reason = msg, code = code, "rejected admin request (csrf)");
        return error_response(StatusCode::FORBIDDEN, code, msg).into_response();
    }
    next.run(request).await
}

/// Axum middleware that resolves the live `Principal` from the user
/// collection and inserts it as a request extension. Must run after
/// [`authenticate_layer`].
pub(crate) async fn resolve_principal_layer(
    State(state): State<Arc<AdminRuntimeState>>,
    mut request: Request,
    next: Next,
) -> axum::response::Response {
    let auth = request
        .extensions()
        .get::<AdminAuth>()
        .expect("authenticate_layer must run before resolve_principal_layer")
        .clone();
    let principal = match resolve_principal(&state, &auth) {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
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

/// Axum middleware (run as `map_response` via `from_fn`) that refreshes
/// the CSRF cookie on every authenticated admin response.
pub(crate) async fn attach_csrf_cookie_layer(
    State(state): State<Arc<AdminRuntimeState>>,
    request: Request,
    next: Next,
) -> axum::response::Response {
    let auth = request
        .extensions()
        .get::<AdminAuth>()
        .expect("authenticate_layer must run before attach_csrf_cookie_layer")
        .clone();
    let mut response = next.run(request).await;
    let cookie = crate::auth::render_csrf_cookie(
        &auth.0.csrf_secret,
        state.session_lifetime_secs,
        state.cookies_secure,
    );
    match hyper::header::HeaderValue::from_str(&cookie) {
        Ok(v) => {
            response.headers_mut().append(hyper::header::SET_COOKIE, v);
        }
        Err(e) => {
            warn!(err = %e, "could not append CSRF cookie header");
        }
    }
    response
}

#[cfg(test)]
mod tests;
