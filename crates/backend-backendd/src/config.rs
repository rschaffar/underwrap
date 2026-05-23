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
    /// Optional allowlist of `Origin` (or `Referer`) values for
    /// state-changing requests. When set, every non-GET/HEAD/OPTIONS
    /// request must carry an Origin matching one of these entries —
    /// the same-origin check that backs CSRF defence.
    #[serde(default)]
    pub allowed_origins: Option<Vec<String>>,
    /// Session lifetime in seconds. Defaults to
    /// `auth::DEFAULT_SESSION_LIFETIME_SECS` (12 hours) when unset.
    #[serde(default)]
    pub session_lifetime_secs: Option<u64>,
    /// Directory under which the admin "create store" API places the
    /// data directories of newly-created dynamic stores. Each store
    /// lives at `<stores_data_root>/<store_id>/`. Unset disables store
    /// creation (returns `data_root_not_configured`).
    #[serde(default)]
    pub stores_data_root: Option<PathBuf>,
}

/// Default location of the backend-managed store-registry file.
pub const DEFAULT_STORES_FILE: &str = "/var/lib/underwrap-backend/stores.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Path of the backend-managed store-registry file. Missing files
    /// are treated as an empty registry. Defaults to
    /// [`DEFAULT_STORES_FILE`].
    #[serde(default)]
    pub stores_file: Option<PathBuf>,
    /// Directory holding the per-instance JSON files that make up the
    /// runtime client-instance registry. Each file is named
    /// `<client_instance_id>.json`. Missing directories are treated as
    /// an empty registry. Defaults to
    /// `/var/lib/underwrap-backend/clients`.
    #[serde(default)]
    pub clients_dir: Option<PathBuf>,
    /// Path of the backend-managed user-state file. Missing files are
    /// treated as an empty user collection with bootstrap incomplete.
    /// Defaults to [`crate::users::DEFAULT_USERS_FILE`].
    #[serde(default)]
    pub users_file: Option<PathBuf>,
    #[serde(default)]
    pub https: Option<HttpsConfig>,
    #[serde(default)]
    pub admin: Option<AdminConfig>,
}

impl BackendConfig {
    /// Resolve [`Self::stores_file`] or the project default.
    pub fn stores_file(&self) -> PathBuf {
        self.stores_file
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STORES_FILE))
    }

    /// Resolve [`Self::clients_dir`] or the project default.
    pub fn clients_dir(&self) -> PathBuf {
        self.clients_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("/var/lib/underwrap-backend/clients"))
    }

    /// Resolve [`Self::users_file`] or the project default.
    pub fn users_file(&self) -> PathBuf {
        self.users_file
            .clone()
            .unwrap_or_else(|| PathBuf::from(crate::users::DEFAULT_USERS_FILE))
    }
}
