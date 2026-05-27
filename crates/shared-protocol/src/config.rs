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

    /// Instance name. Used for debug metadata and (in the NixOS module)
    /// the `uw-<instance>` binary / `underwrap-<instance>` group naming.
    pub instance: String,

    /// The single wrapper socket for this instance. One socket per
    /// instance now selects the daemon; the wrapper name is a
    /// client-supplied selector carried over IPC, not encoded in the
    /// socket path.
    pub socket: PathBuf,

    /// The single setgid group gating `socket` (mode 0660). Acquiring
    /// this group is the sole kernel-enforced gate.
    pub group: String,

    /// Absolute path to the `underwrap-ns` helper the daemon names in
    /// the exec plan.
    pub ns_binary: PathBuf,

    /// Absolute path to the debug tool exec'd for `variant = Debug`.
    /// The daemon emits this only after the backend authorised a
    /// `Debug` invocation, never the wrapper.
    pub debug_tool_target: PathBuf,

    /// Base directory from which the target `XDG_RUNTIME_DIR` is derived
    /// as `<xdg_runtime_dir_base>/<uid>`.
    pub xdg_runtime_dir_base: String,

    /// Wrapper definitions, keyed by the bare wrapper name. The map key
    /// is the canonical name registered with the backend and selected by
    /// clients over IPC.
    #[serde(default)]
    pub wrappers: HashMap<String, WrapperDef>,
}

/// Reserved wrapper-name token: `uw-<instance> debug <wrapper>` selects
/// the debug variant, so a wrapper may not itself be named `debug`.
pub const RESERVED_WRAPPER_NAME_DEBUG: &str = "debug";

impl DaemonConfig {
    /// Validate the instance-level invariants that are independent of the
    /// daemon's runtime environment (group/socket existence is checked at
    /// bind time). Rejects a wrapper named `debug` (reserved subcommand
    /// token) and a wrapper with no projection slot enabled.
    pub fn validate(&self) -> Result<(), String> {
        if self.instance.is_empty() {
            return Err("instance name must not be empty".to_string());
        }
        for (name, wrapper) in &self.wrappers {
            if name == RESERVED_WRAPPER_NAME_DEBUG {
                return Err(format!(
                    "wrapper name '{RESERVED_WRAPPER_NAME_DEBUG}' is reserved \
                     (it is the debug subcommand token)"
                ));
            }
            if !(wrapper.env || wrapper.files.is_some() || wrapper.libsecret) {
                return Err(format!(
                    "wrapper '{name}' enables no projection slot \
                     (set at least one of env/files/libsecret)"
                ));
            }
        }
        Ok(())
    }
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

/// One backend-managed store record. Persisted as one
/// `<stores_dir>/<id>.json` file per store; the id is the file stem
/// (not the JSON body) and `name` is the editable operator-facing
/// label. The loader backfills `id` from the filename.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreRecord {
    /// Stable internal identifier for this store.
    ///
    /// Not serialised: on disk the id is implied by the filename so
    /// it cannot drift between the two.
    #[serde(default, skip)]
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
///
/// The map key in `DaemonConfig::wrappers` is the canonical wrapper name;
/// there is no per-wrapper socket or group any more (one of each per
/// instance). Exec parameters that used to live in a per-wrapper sidecar
/// config (`env_passthrough`, `env_set`) are absorbed here so the daemon
/// can supply the full exec plan in `ReadOk`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WrapperDef {
    /// Absolute path to the target binary to exec for `variant = Real`.
    pub target: PathBuf,

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

    /// User environment variables allowed through to the target.
    #[serde(default)]
    pub env_passthrough: Vec<String>,

    /// Static environment variables injected into the target.
    #[serde(default)]
    pub env_set: HashMap<String, String>,
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
