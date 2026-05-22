//! Browser-facing admin API and its supporting infrastructure.
//!
//! [`admin_router`] composes every URL under `/v1/admin/*` plus the
//! SPA-asset fallback into a single `axum::Router`. Cross-cutting gates
//! — Origin check, session-cookie auth, CSRF double-submit, principal
//! resolve, CSRF cookie refresh — are applied as `tower::Layer`s on the
//! protected sub-router. The public sub-router (login, whoami,
//! bootstrap status, bootstrap) runs only the Origin check.
//!
//! Submodules:
//!
//! - [`audit`] — structured `admin event` log emitter shared by every
//!   subdomain.
//! - [`csrf`] — session-cookie authentication, CSRF double-submit gate,
//!   per-request `Principal` resolver, and CSRF cookie refresher,
//!   exposed both as standalone helpers and as tower layers.
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
use axum::middleware::from_fn_with_state;

use crate::http::origin::origin_layer;
use crate::runtime_state::AdminRuntimeState;

use self::csrf::{
    attach_csrf_cookie_layer, authenticate_layer, check_csrf_layer, resolve_principal_layer,
};

/// Build the admin sub-router for `/v1/admin/*`. The returned router
/// is parameterized by `Arc<AdminRuntimeState>` so the caller composes
/// it with the runtime router and then provides state once via
/// `.with_state(...)`.
pub(crate) fn admin_router(state: Arc<AdminRuntimeState>) -> Router<Arc<AdminRuntimeState>> {
    let public = auth::public_router().merge(bootstrap::public_router());

    // Layer order: `.layer` wraps the *current* router in the supplied
    // middleware, so calls read inside-out. Request flows
    // outer→inner (origin → authenticate → resolve → csrf →
    // attach-cookie-wrapper → handler); response flows back the same
    // chain in reverse, which is why `attach_csrf_cookie_layer` is
    // registered first — it must close over `AdminAuth`, which only
    // exists once `authenticate_layer` has run.
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
        .layer(from_fn_with_state(state.clone(), attach_csrf_cookie_layer))
        .layer(axum::middleware::from_fn(check_csrf_layer))
        .layer(from_fn_with_state(state.clone(), resolve_principal_layer))
        .layer(from_fn_with_state(state.clone(), authenticate_layer));

    public
        .merge(protected)
        .layer(from_fn_with_state(state, origin_layer))
}
