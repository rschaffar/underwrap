//! Browser-facing admin API and its supporting infrastructure.
//!
//! [`admin_router`] composes every URL under `/v1/admin/*` plus the
//! SPA-asset fallback into a single `axum::Router`. The cross-cutting
//! gates are wired as `tower::Layer`s:
//!
//! - Origin / Referer allowlist (state-changing requests) on every
//!   admin URL.
//! - [`tower_sessions::SessionManagerLayer`] + the [`axum_login`]
//!   `AuthManagerLayer` provide the [`AuthSession`] extractor and the
//!   opaque session cookie.
//! - [`axum_csrf::CsrfLayer`] populates a [`CsrfToken`] extension on
//!   every request; the project-local
//!   [`csrf::attach_csrf_response_layer`] writes the corresponding
//!   `Set-Cookie` and `X-CSRF-Token` header onto every response.
//! - State-changing requests on the protected sub-router are gated by
//!   [`csrf::verify_csrf_layer`].
//! - The protected sub-router additionally enforces
//!   [`csrf::require_login_layer`] (canonical 401 envelope) and
//!   [`csrf::resolve_principal_layer`] (live-rename re-read).
//!
//! Submodules:
//!
//! - [`audit`] — structured `admin event` log emitter shared by every
//!   subdomain.
//! - [`csrf`] — CSRF config builder and the cross-cutting auth/CSRF
//!   middleware functions described above.
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

use axum::Router;
use axum::middleware::{from_fn, from_fn_with_state};
use axum_csrf::CsrfLayer;
use axum_login::AuthManagerLayerBuilder;
use tower_sessions::cookie::SameSite;
use tower_sessions::cookie::time::Duration as TimeDuration;
use tower_sessions::{Expiry, SessionManagerLayer};

use crate::http::origin::origin_layer;
use crate::runtime_state::AdminRuntimeState;

use self::csrf::{
    attach_csrf_response_layer, require_login_layer, resolve_principal_layer, verify_csrf_layer,
};

/// Cookie name for the `tower-sessions` session id. `__Host-` forces
/// `Secure; Path=/; no Domain`, which closes subdomain confusion.
pub(crate) const SESSION_COOKIE_NAME: &str = "__Host-uw-session";

/// Build the admin sub-router for `/v1/admin/*`. The returned router
/// is parameterized by `Arc<AdminRuntimeState>` so the caller composes
/// it with the runtime router and provides state once via
/// `.with_state(...)`.
pub(crate) fn admin_router(state: Arc<AdminRuntimeState>) -> Router<Arc<AdminRuntimeState>> {
    // ---- tower-sessions + axum-login ----------------------------------
    let session_lifetime =
        TimeDuration::seconds(i64::try_from(state.session_lifetime_secs).unwrap_or(i64::MAX));
    let session_layer = SessionManagerLayer::new(state.session_store.clone())
        .with_name(SESSION_COOKIE_NAME)
        .with_http_only(true)
        .with_same_site(SameSite::Strict)
        .with_secure(state.cookies_secure)
        .with_path("/")
        .with_expiry(Expiry::OnInactivity(session_lifetime));
    let auth_layer =
        AuthManagerLayerBuilder::new(state.auth_backend.clone(), session_layer).build();

    // ---- axum_csrf ----------------------------------------------------
    let csrf_layer = CsrfLayer::new(state.csrf_config.clone());

    // ---- routers ------------------------------------------------------
    let public = auth::public_router().merge(bootstrap::public_router());

    // `.layer` wraps the current router with the supplied middleware,
    // so layers read inside-out — the *first* `.layer(...)` call is
    // closest to the handler. For the protected sub-router we need:
    //
    //   request:  require_login → verify_csrf → resolve_principal → handler
    //   response: handler        → resolve_principal → verify_csrf → require_login
    //
    // …which means we register them bottom-up in code: resolve first
    // (innermost), then verify, then require_login (outermost).
    let protected = Router::new()
        .merge(auth::protected_router())
        .merge(users::router())
        .merge(stores::router())
        .merge(client_instances::router())
        .merge(secrets::router())
        .merge(resources::router())
        .merge(pairs::router())
        .merge(policy::router())
        .merge(overview::router())
        .layer(from_fn_with_state(state.clone(), resolve_principal_layer))
        .layer(from_fn(verify_csrf_layer))
        .layer(from_fn(require_login_layer));

    // The outer composition. `attach_csrf_response_layer` and
    // `csrf_layer` apply to *both* sub-routers so the SPA always sees
    // a fresh `X-CSRF-Token` even on public endpoints (login,
    // whoami). The auth layer must also wrap both because
    // `handle_admin_login` calls `auth_session.login(&user)`.
    public
        .merge(protected)
        .layer(from_fn(attach_csrf_response_layer))
        .layer(csrf_layer)
        .layer(auth_layer)
        .layer(from_fn_with_state(state, origin_layer))
}
