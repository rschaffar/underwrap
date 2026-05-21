//! Browser-facing admin API and its supporting infrastructure.
//!
//! `handle_admin_request` (still in `server.rs` at this point in the
//! modularization epic) is the URL dispatcher. The submodules here
//! own:
//!
//! - [`audit`] — structured `admin event` log emitter shared by every
//!   subdomain.
//! - [`csrf`] — session-cookie authentication + CSRF double-submit
//!   gate + per-request `Principal` resolver + `Set-Cookie` refresher.
//!
//! Per-subdomain handler modules (`auth`, `bootstrap`, `users`,
//! `stores`, etc.) will land in later modularization steps.

pub(crate) mod audit;
pub mod auth;
pub mod bootstrap;
pub(crate) mod csrf;
pub(crate) mod overview;
pub(crate) mod pairs;
pub(crate) mod policy;
pub(crate) mod resources;
pub(crate) mod secrets;
pub(crate) mod users;
