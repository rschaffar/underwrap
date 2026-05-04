//! Configuration schema — maps to the runtime YAML config loaded by underwrapd.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Backend service the frontend talks to.
    pub backend: BackendServiceConfig,

    /// Host label published into the backend catalog for this frontend.
    #[serde(default = "default_catalog_host")]
    pub catalog_host: String,

    /// Frontend instance label published into the backend catalog.
    #[serde(default)]
    pub catalog_instance: String,

    /// Usernames allowed to execute wrappers in this instance.
    /// Empty = allow all local users.
    #[serde(default)]
    pub allowed_users: Vec<String>,

    /// Wrapper definitions, keyed by wrapper name.
    #[serde(default)]
    pub wrappers: HashMap<String, WrapperDef>,
}

/// Storage backend configuration — how `underwrap-backendd` persists secrets
/// behind the service API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "backend")]
pub enum StorageConfig {
    /// Plain directory of files owned by the daemon user.
    #[serde(rename = "plain")]
    Plain {
        #[serde(default = "default_storage_dir")]
        dir: PathBuf,
    },
    /// Encrypted directory: secrets encrypted at rest with a master key.
    /// The master key file must be readable only by the daemon user.
    #[serde(rename = "encrypted")]
    Encrypted {
        #[serde(default = "default_storage_dir")]
        dir: PathBuf,
        /// Path to the master key file (32 bytes, raw or hex-encoded).
        key_file: PathBuf,
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

fn default_storage_dir() -> PathBuf {
    PathBuf::from("/var/lib/underwrap/secrets")
}

fn default_catalog_host() -> String {
    "unknown-host".to_string()
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
pub struct WrapperDef {
    /// Absolute path to the target binary to exec.
    pub target: PathBuf,

    /// Human-facing wrapper name used for backend catalog registration.
    /// Defaults to the wrapper map key when omitted.
    #[serde(default)]
    pub logical_name: Option<String>,

    /// Resource projections for this wrapper, keyed by backend resource id.
    ///
    /// A resource always has all three backend slots (`env`, `files`,
    /// `libsecret`). This map only decides which slots the frontend projects
    /// into the wrapped process. Read/write authority is decided solely by the
    /// backend policy document.
    #[serde(default)]
    pub resources: HashMap<String, ResourceUse>,

    /// Unix socket path for this wrapper's authenticated daemon channel.
    pub socket: PathBuf,

    /// Unix group name for GID-based wrapper identity.
    pub group: String,
}

/// Frontend projection of one backend resource into a wrapper invocation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ResourceUse {
    /// Project every variable configured in `<resource>/env/*` into the
    /// target environment.
    #[serde(default)]
    pub env: bool,

    /// Project `<resource>/files/*` as a namespace-private directory mounted
    /// at `path` relative to the target user's home.
    #[serde(default)]
    pub files: Option<FilesUse>,

    /// Project `<resource>/libsecret/*` through a private Secret Service.
    #[serde(default)]
    pub libsecret: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FilesUse {
    /// Target-home-relative path for the files slot.
    pub path: String,
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
mod tests {
    use super::*;

    #[test]
    fn minimal_config() {
        let yaml = r#"
backend:
  url: https://backend.example.internal:8443
  ca_file: /etc/underwrap/ca.pem
  client_cert_file: /etc/underwrap/client.pem
  client_key_file: /etc/underwrap/client.key
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.wrappers.is_empty());
        assert_eq!(config.catalog_host, "unknown-host");
        assert_eq!(config.catalog_instance, "");
        assert_eq!(config.backend.url, "https://backend.example.internal:8443");
        assert_eq!(
            config.backend.ca_file,
            Some(PathBuf::from("/etc/underwrap/ca.pem"))
        );
        assert_eq!(
            config.backend.client_cert_file,
            Some(PathBuf::from("/etc/underwrap/client.pem"))
        );
        assert_eq!(
            config.backend.client_key_file,
            Some(PathBuf::from("/etc/underwrap/client.key"))
        );
    }

    #[test]
    fn full_wrapper_config() {
        let yaml = r#"
backend:
  url: https://backend.example.internal:8443
  ca_file: /etc/underwrap/ca.pem
  client_cert_file: /etc/underwrap/client.pem
  client_key_file: /etc/underwrap/client.key
catalog_host: workstation-01
catalog_instance: robert
wrappers:
  gh-authorized:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-authorized.sock
    group: underwrap-gh-authorized
    resources:
      github/token:
        env: true
      gws/state:
        files:
          path: .config/gws
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.wrappers.len(), 1);
        assert_eq!(config.catalog_host, "workstation-01");
        assert_eq!(config.catalog_instance, "robert");
        let w = &config.wrappers["gh-authorized"];
        assert_eq!(w.target, PathBuf::from("/usr/bin/gh"));
        assert_eq!(w.resources.len(), 2);
        assert!(w.resources["github/token"].env);
        assert_eq!(
            w.resources["gws/state"].files.as_ref().unwrap().path,
            ".config/gws"
        );
    }

    #[test]
    fn resource_use_rejects_unknown_fields() {
        let yaml = r#"
backend:
  url: https://x
  ca_file: /x
  client_cert_file: /y
  client_key_file: /z
wrappers:
  bad:
    target: /usr/bin/true
    socket: /run/x.sock
    group: g
    resources:
      github/token:
        variable: GH_TOKEN
"#;
        let err = serde_yaml::from_str::<DaemonConfig>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown field `variable`"));
    }

    #[test]
    fn files_projection_requires_path() {
        let yaml = r#"
backend:
  url: https://x
  ca_file: /x
  client_cert_file: /y
  client_key_file: /z
wrappers:
  bad:
    target: /usr/bin/true
    socket: /run/x.sock
    group: g
    resources:
      gws/state:
        files: {}
"#;
        let err = serde_yaml::from_str::<DaemonConfig>(yaml).unwrap_err();
        assert!(err.to_string().contains("missing field `path`"));
    }
}
