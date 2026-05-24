pub(crate) mod admin_api;
pub mod auth;
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
pub mod tls;
pub mod users;
pub mod web;

// Public-API surface. External consumers (integration-tests, the
// `ts_export` binding test, and the `underwrap-backendd` binary itself)
// reach for these types via the crate root.
pub use crate::admin_api::admin_openapi_spec;
pub use crate::config::{
    AdminConfig, BackendConfig, DEFAULT_STORES_DIR, HttpsConfig, HttpsTlsConfig,
};
pub use crate::store_registry::{
    RegistryLookupError, StoreEntry, StoreRegistry, StoreState, load_store_records,
};
