//! Configuration schema — maps to the runtime YAML config loaded by underwrapd.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub use crate::access::AccessMode;

// A wrapper projects exactly one backend resource. The resource may opt
// into any combination of the three projection slots (`env`, `files`,
// `libsecret`); at least one must be enabled. The backend pair table is
// still keyed by `(wrapper, resource)` so two frontend instances can map
// the same wrapper name to different resources, and the grant key keeps
// `resource` so a frontend-side config edit cannot redirect an existing
// grant to a different secret.

/// Top-level daemon configuration.
///
/// The backend assigns the frontend's externally meaningful identity at
/// enrollment and binds it to the runtime mTLS client certificate. Per-principal
/// authorization is expressed by backend permission rows keyed on that runtime
/// client instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Backend service the frontend talks to.
    pub backend: BackendServiceConfig,

    /// Instance name. Used for debug metadata and (in the NixOS module)
    /// the `underwrap-<instance>` binary and same-named instance group.
    pub instance: String,

    /// The single wrapper socket for this instance. The socket selects the
    /// daemon; the wrapper name is a client-supplied selector carried over IPC.
    pub socket: PathBuf,

    /// The single setgid group gating `socket` (mode 0660). Acquiring
    /// this group is the sole kernel-enforced gate.
    pub group: String,

    /// Absolute path to the debug tool exec'd for `variant = Debug`.
    /// The daemon emits this only after the backend authorised a
    /// `Debug` invocation, never the wrapper.
    pub debug_tool_target: PathBuf,

    /// Base directory under which each isolated target's own runtime
    /// directory is provisioned by the system. The default target
    /// `XDG_RUNTIME_DIR` is derived as
    /// `<target_runtime_dir_base>/<target-user-name>` — the executing
    /// dedicated uid's own dir, not the caller's.
    pub target_runtime_dir_base: String,

    /// Wrapper definitions, keyed by the bare wrapper name. The map key
    /// is the canonical name registered with the backend and selected by
    /// clients over IPC.
    #[serde(default)]
    pub wrappers: HashMap<String, WrapperDef>,
}

/// Reserved wrapper-name token: `underwrap-<instance> debug <wrapper>` selects
/// the debug variant, so a wrapper may not itself be named `debug`.
pub const RESERVED_WRAPPER_NAME_DEBUG: &str = "debug";

fn is_instance_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=21).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_-".contains(byte))
}

fn is_wrapper_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

fn has_forbidden_path_character(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
}

fn is_canonical_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !has_forbidden_path_character(value)
        && value
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

/// Return whether `path` uses the shared canonical absolute lexical grammar.
pub fn is_canonical_absolute_path(path: &std::path::Path) -> bool {
    let Some(value) = path.to_str() else {
        return false;
    };
    value == "/"
        || value
            .strip_prefix('/')
            .is_some_and(is_canonical_relative_path)
}

fn paths_overlap(left: &str, right: &str) -> bool {
    left == right
        || (left == "/" && right.starts_with('/'))
        || (right == "/" && left.starts_with('/'))
        || right
            .strip_prefix(left)
            .is_some_and(|suffix| suffix.starts_with('/'))
        || left
            .strip_prefix(right)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AccessPathDomain {
    Absolute,
    Tilde,
}

fn access_path_domain(value: &str) -> Option<AccessPathDomain> {
    if is_canonical_absolute_path(std::path::Path::new(value)) {
        Some(AccessPathDomain::Absolute)
    } else if value == "~"
        || value
            .strip_prefix("~/")
            .is_some_and(is_canonical_relative_path)
    {
        Some(AccessPathDomain::Tilde)
    } else {
        None
    }
}

impl DaemonConfig {
    /// Validate lexical configuration invariants before any filesystem lookup,
    /// socket bind, credential read, or wrapper invocation.
    pub fn validate(&self) -> Result<(), String> {
        use crate::target_env::{CONFIGURABLE_FORCED_TARGET_ENV_NAMES, is_forced_target_env_name};

        if !is_instance_name(&self.instance) {
            return Err("instance name must match [a-z][a-z0-9_-]{0,20}".to_string());
        }

        for (field, path) in [
            ("socket", self.socket.as_path()),
            ("debug_tool_target", self.debug_tool_target.as_path()),
            (
                "target_runtime_dir_base",
                std::path::Path::new(&self.target_runtime_dir_base),
            ),
        ] {
            if !is_canonical_absolute_path(path) {
                return Err(format!("{field} must be a canonical absolute path"));
            }
        }
        for (field, path) in [
            ("backend.ca_file", self.backend.ca_file.as_deref()),
            (
                "backend.client_identity_file",
                self.backend.client_identity_file.as_deref(),
            ),
        ] {
            if path.is_some_and(|path| !is_canonical_absolute_path(path)) {
                return Err(format!("{field} must be a canonical absolute path"));
            }
        }

        let mut wrappers = self.wrappers.iter().collect::<Vec<_>>();
        wrappers.sort_unstable_by_key(|(name, _)| *name);
        for (name, wrapper) in wrappers {
            if !is_wrapper_name(name) {
                return Err("wrapper name must match [A-Za-z0-9][A-Za-z0-9._+-]{0,127}".to_string());
            }
            if name == RESERVED_WRAPPER_NAME_DEBUG {
                return Err(format!(
                    "wrapper name '{RESERVED_WRAPPER_NAME_DEBUG}' is reserved \
                     (it is the debug subcommand token)"
                ));
            }
            if !is_canonical_absolute_path(&wrapper.target) {
                return Err(format!(
                    "wrapper '{name}' target must be a canonical absolute path"
                ));
            }
            if !(wrapper.env || wrapper.files.is_some() || wrapper.libsecret) {
                return Err(format!(
                    "wrapper '{name}' enables no projection slot \
                     (set at least one of env/files/libsecret)"
                ));
            }
            if let Some(files) = &wrapper.files {
                if files.paths.is_empty() {
                    return Err(format!(
                        "wrapper '{name}' files.paths must contain at least one path"
                    ));
                }
                if !files
                    .paths
                    .iter()
                    .all(|path| is_canonical_relative_path(path))
                {
                    return Err(format!(
                        "wrapper '{name}' files.paths entries must be canonical relative paths"
                    ));
                }
                if files.paths.iter().enumerate().any(|(index, left)| {
                    files.paths[index + 1..]
                        .iter()
                        .any(|right| paths_overlap(left, right))
                }) {
                    return Err(format!(
                        "wrapper '{name}' files.paths must be unique and non-overlapping"
                    ));
                }
            }

            let access_domains = wrapper
                .access_paths
                .iter()
                .map(|path| access_path_domain(path))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    format!(
                        "wrapper '{name}' access_paths entries must be canonical absolute paths, \
                         '~', or canonical '~/...' paths"
                    )
                })?;
            if wrapper
                .access_paths
                .iter()
                .enumerate()
                .any(|(index, left)| {
                    wrapper.access_paths[index + 1..]
                        .iter()
                        .enumerate()
                        .any(|(offset, right)| {
                            access_domains[index] == access_domains[index + 1 + offset]
                                && paths_overlap(left, right)
                        })
                })
            {
                return Err(format!(
                    "wrapper '{name}' access_paths must be unique and non-overlapping \
                     within the absolute and tilde domains"
                ));
            }

            // The target inherits `env_passthrough` from the caller's
            // environment, so it must never carry an env name that steers
            // where the target finds config/cache/runtime dirs. Those are
            // forced by the wrapper chain; allowing passthrough would let a
            // caller redirect the target's view of its own state.
            for passthrough in &wrapper.env_passthrough {
                if is_forced_target_env_name(passthrough) {
                    if passthrough == "XDG_RUNTIME_DIR" {
                        return Err(format!(
                            "wrapper '{name}' env_passthrough must not include \
                             XDG_RUNTIME_DIR; use target_runtime_dir_base instead"
                        ));
                    }
                    return Err(format!(
                        "wrapper '{name}' env_passthrough must not include \
                         {passthrough}; use env_set for trusted HOME/XDG overrides"
                    ));
                }
            }
            if wrapper.env_set.contains_key("XDG_RUNTIME_DIR") {
                return Err(format!(
                    "wrapper '{name}' env_set must not include XDG_RUNTIME_DIR; \
                     use target_runtime_dir_base instead"
                ));
            }
            for forced in CONFIGURABLE_FORCED_TARGET_ENV_NAMES {
                if let Some(value) = wrapper.env_set.get(*forced)
                    && !is_canonical_absolute_path(std::path::Path::new(value))
                {
                    return Err(format!(
                        "wrapper '{name}' env_set {forced} must be a canonical absolute path"
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Storage backend configuration — how `underwrap-backend` persists secrets
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
#[serde(deny_unknown_fields)]
pub struct BackendServiceConfig {
    /// Backend service URL, e.g. `https://backend.example.internal:8443`.
    pub url: String,

    /// CA certificate bundle used to verify the backend service certificate.
    pub ca_file: Option<PathBuf>,

    /// Combined client certificate-chain and private-key PEM used for mTLS.
    pub client_identity_file: Option<PathBuf>,
}

/// A wrapper definition.
///
/// The map key in `DaemonConfig::wrappers` is the canonical wrapper name. Each
/// instance has one socket and group, while the daemon supplies this wrapper's
/// complete execution plan in `ReadOk`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WrapperDef {
    /// Absolute path to the target binary to exec for `variant = Real`.
    pub target: PathBuf,

    /// Operator-facing backend resource name this wrapper projects into the target.
    /// The backend resolves this name to the stable generated resource id.
    /// Each wrapper projects exactly one resource.
    /// The resource always has all three backend slots (`env`, `files`, `libsecret`) available.
    /// The wrapper opts into the subset it needs via the flags below.
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

    /// Caller-owned directories to make available to the dedicated-UID target
    /// via idmapped bind mounts that map only the caller's UID/GID to the
    /// target's. Each entry is `~`, `~/<rel>` (resolved against the caller's
    /// home), or an absolute path. Other file owners remain unmapped.
    #[serde(default)]
    pub access_paths: Vec<String>,
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

#[cfg(test)]
mod tests;
