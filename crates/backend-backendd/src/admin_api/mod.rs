//! Browser-facing admin API and its supporting infrastructure.
//!
//! [`handle_admin_request`] is the dispatcher for every URL under
//! `/v1/admin/*` plus the SPA-asset fallback. It runs the cross-cutting
//! gates — SPA fallback, Origin check, public auth/bootstrap routes,
//! session-cookie auth, CSRF double-submit, principal resolve, logout
//! — and then delegates per-subdomain routing to each module's
//! [`route`] function.
//!
//! Submodules:
//!
//! - [`audit`] — structured `admin event` log emitter shared by every
//!   subdomain.
//! - [`csrf`] — session-cookie authentication + CSRF double-submit
//!   gate + per-request `Principal` resolver + `Set-Cookie` refresher.
//! - [`auth`] / [`bootstrap`] — login, whoami, logout, password change,
//!   first-operator bootstrap.
//! - [`users`], [`stores`], [`client_instances`], [`secrets`],
//!   [`policy`], [`resources`], [`pairs`], [`overview`] — admin CRUD
//!   per subdomain.

pub(crate) mod audit;
pub mod auth;
pub mod bootstrap;
pub(crate) mod client_instances;
pub(crate) mod csrf;
pub(crate) mod overview;
pub(crate) mod pairs;
pub(crate) mod policy;
pub(crate) mod resources;
pub(crate) mod secrets;
pub(crate) mod stores;
pub(crate) mod users;

use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use tracing::{info, warn};

use crate::http::origin::check_admin_origin;
use crate::http::responses::error_response;
use crate::runtime_state::AdminRuntimeState;

use self::auth::{handle_admin_login, handle_admin_logout, handle_admin_whoami};
use self::bootstrap::{handle_admin_bootstrap, handle_admin_bootstrap_status};
use self::csrf::{
    AdminAuthError, attach_csrf_cookie, authenticate_admin, check_csrf, resolve_principal,
};

pub(crate) async fn handle_admin_request(
    request: Request<Incoming>,
    state: Arc<AdminRuntimeState>,
) -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible> {
    // 1. SPA assets — served from the embedded bundle (or dev source)
    // without running the API gates so the login page itself can load.
    if crate::web::is_spa_request(request.method(), request.uri().path()) {
        return Ok(crate::web::serve_web_request(
            request.method(),
            request.uri().path(),
            &state.web_source,
        ));
    }

    // 2. Origin gate (applies to every JSON endpoint, public or not).
    if let Err(reason) = check_admin_origin(
        request.method(),
        request.headers(),
        state.allowed_origins.as_deref(),
    ) {
        warn!(reason = reason, "rejected admin request (origin check)");
        return Ok(error_response(StatusCode::FORBIDDEN, "bad_origin", reason));
    }

    // 3. Public auth + bootstrap surfaces. They exist *to* establish a
    // session, so they run before the session-cookie check.
    match (request.method(), request.uri().path()) {
        (&Method::POST, "/v1/admin/auth/login") => {
            return Ok(handle_admin_login(&state, request).await);
        }
        (&Method::GET, "/v1/admin/auth/whoami") => {
            return Ok(handle_admin_whoami(&state, request.headers()));
        }
        (&Method::GET, "/v1/admin/auth/bootstrap/status") => {
            return Ok(handle_admin_bootstrap_status(&state));
        }
        (&Method::POST, "/v1/admin/auth/bootstrap") => {
            return Ok(handle_admin_bootstrap(&state, request).await);
        }
        _ => {}
    }

    // 4. Session-cookie auth.
    let auth = match authenticate_admin(&state, request.headers()) {
        Ok(a) => a,
        Err(AdminAuthError::Revoked) => {
            warn!("rejected admin request: session revoked");
            return Ok(error_response(
                StatusCode::UNAUTHORIZED,
                "session_revoked",
                "session has been revoked",
            ));
        }
        Err(AdminAuthError::Unauthenticated(reason)) => {
            warn!(reason = reason, "rejected admin request");
            return Ok(error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                reason,
            ));
        }
    };

    // 5. CSRF double-submit on state-changing verbs.
    if let Err((code, msg)) = check_csrf(
        request.method(),
        request.headers(),
        &auth.session().csrf_secret,
    ) {
        warn!(reason = msg, code = code, "rejected admin request (csrf)");
        return Ok(error_response(StatusCode::FORBIDDEN, code, msg));
    }

    // 6. Resolve the live `Principal` from the user collection.
    let principal = match resolve_principal(&state, &auth) {
        Ok(p) => p,
        Err(resp) => return Ok(resp),
    };
    info!(
        user = %principal.username,
        method = %request.method(),
        path = request.uri().path(),
        "admin request"
    );

    // 7. Logout runs *after* auth + CSRF have passed so a drive-by
    // cookie-only request can't revoke someone else's session.
    if request.method() == Method::POST && request.uri().path() == "/v1/admin/auth/logout" {
        return Ok(handle_admin_logout(&state, auth.session()));
    }

    // 8. Dispatch by URL prefix to each subdomain's `route` function.
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let response = if path == "/v1/admin/auth/password" {
        auth::route(&method, &path, &state, &principal, request).await
    } else if path == "/v1/admin/users" || path.starts_with("/v1/admin/users/") {
        users::route(&method, &path, &state, &principal, request).await
    } else if path == "/v1/admin/stores" || path.starts_with("/v1/admin/stores/") {
        stores::route(&method, &path, &state, &principal, request).await
    } else if path == "/v1/admin/client-instances"
        || path.starts_with("/v1/admin/client-instances/")
    {
        client_instances::route(&method, &path, &state, &principal, request).await
    } else if path.starts_with("/v1/admin/secret/") {
        secrets::route(&method, &path, &state, &principal, request).await
    } else if path.starts_with("/v1/admin/resources/") {
        resources::route(&method, &path, &state, &principal, request).await
    } else if path.starts_with("/v1/admin/pairs/") {
        pairs::route(&method, &path, &state, &principal, request).await
    } else if path.starts_with("/v1/admin/policy/")
        || path.starts_with("/v1/admin/wrapper-permissions/")
    {
        policy::route(&method, &path, &state, &principal, request).await
    } else if path == "/v1/admin/ui/overview" {
        overview::route(&method, &path, &state, &principal, request).await
    } else {
        error_response(StatusCode::NOT_FOUND, "not_found", "unknown admin endpoint")
    };

    // 9. Refresh the CSRF cookie so the SPA always has a value matching
    // the live session.
    Ok(attach_csrf_cookie(response, &state, &auth))
}
