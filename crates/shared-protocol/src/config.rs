//! Configuration schema — maps to the runtime YAML config loaded by underwrapd.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// A wrapper projects exactly one backend resource. The resource may opt
// into any combination of the three projection slots (`env`, `files`,
// `libsecret`); at least one must be enabled. The backend pair table is
// still keyed by `(wrapper, resource)` so two frontend instances can map
// the same wrapper name to different resources, and the grant key keeps
// `resource` so a frontend-side config edit cannot redirect an existing
// grant to a different secret.

/// Top-level daemon configuration.
///
/// The frontend's only externally meaningful identity is its
/// mTLS-bound runtime client instance id, which the backend assigns
/// at enrollment time. No client-side `host` / `instance` / allowlist
/// fields exist any more — anything that needs per-principal gating
/// is expressed as a backend grant keyed on the client instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Backend service the frontend talks to.
    pub backend: BackendServiceConfig,

    /// Wrapper definitions, keyed by wrapper name.
    #[serde(default)]
    pub wrappers: HashMap<String, WrapperDef>,
}

/// Storage backend configuration — how `underwrap-backendd` persists secrets
/// behind the service API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "backend")]
pub enum StorageConfig {
    /// Plain directory of files owned by the daemon user.
    #[serde(rename = "plain")]
    Plain {
        #[serde(default = "default_storage_dir")]
        dir: PathBuf,
    },
    /// Encrypted directory: secrets encrypted at rest with a 32-byte
    /// master key. The master key lives in process memory only — it is
    /// generated once at store creation, returned to the operator once,
    /// and never written to disk. Restarting the backend leaves every
    /// encrypted store locked until an operator unlocks it via the
    /// admin API.
    #[serde(rename = "encrypted")]
    Encrypted {
        #[serde(default = "default_storage_dir")]
        dir: PathBuf,
    },
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self::Plain {
            dir: default_storage_dir(),
        }
    }
}

impl StorageConfig {
    /// Get the storage directory regardless of backend type.
    pub fn dir(&self) -> &PathBuf {
        match self {
            Self::Plain { dir } | Self::Encrypted { dir, .. } => dir,
        }
    }
}

/// One backend-managed store record in the central store registry.
/// `id` is the stable internal identifier used by APIs/runtime
/// wiring; `name` is the editable operator-facing label.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreRecord {
    /// Stable internal identifier for this store.
    pub id: String,
    /// Editable human-visible store name.
    pub name: String,
    /// Operator-facing description shown in the admin UI. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Unix-epoch seconds the record was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    /// The storage backend itself.
    #[serde(flatten)]
    pub storage: StorageConfig,
}

/// Backend-managed store registry persisted by `underwrap-backendd`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreRegistryDocument {
    #[serde(default)]
    pub stores: Vec<StoreRecord>,
}

fn default_storage_dir() -> PathBuf {
    PathBuf::from("/var/lib/underwrap/secrets")
}

/// Frontend backend-client configuration.
///
/// The frontend talks to a central backend service over HTTPS with mTLS.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BackendServiceConfig {
    /// Backend service URL, e.g. `https://backend.example.internal:8443`.
    pub url: String,

    /// CA certificate bundle used to verify the backend service certificate.
    pub ca_file: Option<PathBuf>,

    /// Client certificate file used for mTLS.
    pub client_cert_file: Option<PathBuf>,

    /// Client private key file used for mTLS.
    pub client_key_file: Option<PathBuf>,
}

/// A wrapper definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WrapperDef {
    /// Absolute path to the target binary to exec.
    pub target: PathBuf,

    /// Human-facing wrapper name used for backend catalog registration.
    /// Defaults to the wrapper map key when omitted.
    #[serde(default)]
    pub logical_name: Option<String>,

    /// Backend resource id this wrapper projects into the target. Each
    /// wrapper projects exactly one resource; the resource itself
    /// always has all three backend slots (`env`, `files`, `libsecret`)
    /// available, and the wrapper opts into the subset it needs via the
    /// flags below.
    pub resource: String,

    /// Project `<resource>/env/*` into the target environment.
    #[serde(default)]
    pub env: bool,

    /// Project `<resource>/files/*` as namespace-private directories
    /// mounted at the given paths relative to the target user's home.
    #[serde(default)]
    pub files: Option<FilesUse>,

    /// Project `<resource>/libsecret/*` through a private Secret Service.
    #[serde(default)]
    pub libsecret: bool,

    /// Unix socket path for this wrapper's authenticated daemon channel.
    pub socket: PathBuf,

    /// Unix group name for GID-based wrapper identity.
    pub group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FilesUse {
    /// Target-home-relative mount paths for the files slot. One backend
    /// resource may project at multiple paths inside the namespace; they
    /// all share a single daemon-managed staging directory and channel,
    /// so reads observe identical content and writes are serialized
    /// through one writeback path regardless of which mount the target
    /// touched. Must contain at least one entry.
    pub paths: Vec<String>,
}

/// Local capability derived from backend policy for a projected mutable slot.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccessMode {
    #[serde(rename = "read-only")]
    ReadOnly,
    #[serde(rename = "read-write")]
    ReadWrite,
}

#[cfg(test)]
mod tests;
