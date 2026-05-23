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

/// Default location of the backend-managed stores directory.
pub const DEFAULT_STORES_DIR: &str = "/var/lib/underwrap-backend/stores";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Directory holding the per-store JSON files that make up the
    /// store registry (`<stores_dir>/<id>.json`). Missing directories
    /// are treated as an empty registry. Defaults to
    /// [`DEFAULT_STORES_DIR`].
    #[serde(default)]
    pub stores_dir: Option<PathBuf>,
    /// Directory holding the per-instance JSON files that make up the
    /// runtime client-instance registry. Each file is named
    /// `<client_instance_id>.json`. Missing directories are treated as
    /// an empty registry. Defaults to
    /// `/var/lib/underwrap-backend/clients`.
    #[serde(default)]
    pub clients_dir: Option<PathBuf>,
    /// Directory holding the per-user JSON files that make up the
    /// admin operator collection (`<users_dir>/<id>.json`). Missing
    /// directories are treated as an empty collection. Defaults to
    /// [`crate::users::DEFAULT_USERS_DIR`].
    #[serde(default)]
    pub users_dir: Option<PathBuf>,
    /// Path of the persistent bootstrap-completed sentinel. The file's
    /// presence is the "bootstrap has run" signal; its absence means
    /// the bootstrap page must be served. Defaults to
    /// [`crate::users::DEFAULT_BOOTSTRAP_FILE`].
    #[serde(default)]
    pub bootstrap_file: Option<PathBuf>,
    #[serde(default)]
    pub https: Option<HttpsConfig>,
    #[serde(default)]
    pub admin: Option<AdminConfig>,
}

impl BackendConfig {
    /// Resolve [`Self::stores_dir`] or the project default.
    pub fn stores_dir(&self) -> PathBuf {
        self.stores_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STORES_DIR))
    }

    /// Resolve [`Self::clients_dir`] or the project default.
    pub fn clients_dir(&self) -> PathBuf {
        self.clients_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("/var/lib/underwrap-backend/clients"))
    }

    /// Resolve [`Self::users_dir`] or the project default.
    pub fn users_dir(&self) -> PathBuf {
        self.users_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(crate::users::DEFAULT_USERS_DIR))
    }

    /// Resolve [`Self::bootstrap_file`] or the project default.
    pub fn bootstrap_file(&self) -> PathBuf {
        self.bootstrap_file
            .clone()
            .unwrap_or_else(|| PathBuf::from(crate::users::DEFAULT_BOOTSTRAP_FILE))
    }
}
