pub(crate) mod admin_api;
pub(crate) mod admin_sessions;
pub(crate) mod auth;
pub mod cli;
pub mod config;
pub(crate) mod http;
pub(crate) mod ids;
pub mod openapi;
pub(crate) mod runtime_api;
pub mod runtime_clients;
pub(crate) mod runtime_state;
pub mod server;
pub(crate) mod storage;
pub mod store_registry;
#[cfg(test)]
pub(crate) mod test_support;
pub(crate) mod time;
pub(crate) mod tls;
pub(crate) mod users;
pub(crate) mod web;

// Public-API surface — the items the integration-tests crate, the
// `ts_export` binding test, and the `underwrap-backend` binary itself
// reach for through the crate root. Everything else is `pub(crate)`
// and stays inside the daemon.
pub use crate::admin_api::admin_openapi_spec;
pub use crate::auth::DEFAULT_SESSION_LIFETIME_SECS;
pub use crate::config::{AdminConfig, BackendConfig, HttpsConfig, HttpsTlsConfig};
