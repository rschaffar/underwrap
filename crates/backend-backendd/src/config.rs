//! Backend daemon configuration types.
//!
//! `BackendConfig` is what the daemon reads from disk at startup;
//! `AdminConfig` and `HttpsConfig` are its nested blocks. Pure data —
//! no behaviour beyond default-resolution helpers.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpsTlsConfig {
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpsConfig {
    pub listen: String,
    pub tls: HttpsTlsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    /// Allowlist of `Origin` (or `Referer`) values for state-changing
    /// requests. When non-empty, every non-GET/HEAD/OPTIONS request
    /// must carry an Origin matching one of these entries — the
    /// same-origin check that backs CSRF defence. Empty disables the
    /// check.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Session lifetime in seconds. Defaults to
    /// `auth::DEFAULT_SESSION_LIFETIME_SECS` (12 hours).
    #[serde(default = "default_session_lifetime_secs")]
    pub session_lifetime_secs: u64,
    /// Directory under which the admin "create store" API places the
    /// data directories of newly-created dynamic stores. Each store
    /// lives at `<stores_data_root>/<store_id>/`. Unset disables store
    /// creation (returns `data_root_not_configured`).
    #[serde(default)]
    pub stores_data_root: Option<PathBuf>,
}

/// Default location of the backend-managed stores directory.
pub const DEFAULT_STORES_DIR: &str = "/var/lib/underwrap-backend/stores";

/// Default location of the runtime client-instance registry directory.
pub const DEFAULT_CLIENTS_DIR: &str = "/var/lib/underwrap-backend/clients";

fn default_stores_dir() -> PathBuf {
    PathBuf::from(DEFAULT_STORES_DIR)
}

fn default_clients_dir() -> PathBuf {
    PathBuf::from(DEFAULT_CLIENTS_DIR)
}

fn default_users_dir() -> PathBuf {
    PathBuf::from(crate::users::DEFAULT_USERS_DIR)
}

fn default_bootstrap_file() -> PathBuf {
    PathBuf::from(crate::users::DEFAULT_BOOTSTRAP_FILE)
}

fn default_session_lifetime_secs() -> u64 {
    crate::auth::DEFAULT_SESSION_LIFETIME_SECS
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Directory holding the per-store JSON files that make up the
    /// store registry (`<stores_dir>/<id>.json`). Missing directories
    /// are treated as an empty registry. Defaults to
    /// [`DEFAULT_STORES_DIR`].
    #[serde(default = "default_stores_dir")]
    pub stores_dir: PathBuf,
    /// Directory holding the per-instance JSON files that make up the
    /// runtime client-instance registry. Each file is named
    /// `<client_instance_id>.json`. Missing directories are treated as
    /// an empty registry. Defaults to [`DEFAULT_CLIENTS_DIR`].
    #[serde(default = "default_clients_dir")]
    pub clients_dir: PathBuf,
    /// Directory holding the per-user JSON files that make up the
    /// admin operator collection (`<users_dir>/<id>.json`). Missing
    /// directories are treated as an empty collection. Defaults to
    /// [`crate::users::DEFAULT_USERS_DIR`].
    #[serde(default = "default_users_dir")]
    pub users_dir: PathBuf,
    /// Path of the persistent bootstrap-completed sentinel. The file's
    /// presence is the "bootstrap has run" signal; its absence means
    /// the bootstrap page must be served. Defaults to
    /// [`crate::users::DEFAULT_BOOTSTRAP_FILE`].
    #[serde(default = "default_bootstrap_file")]
    pub bootstrap_file: PathBuf,
    #[serde(default)]
    pub https: Option<HttpsConfig>,
    #[serde(default)]
    pub admin: Option<AdminConfig>,
}
