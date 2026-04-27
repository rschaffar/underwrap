//! Configuration schema — maps to the runtime YAML config loaded by underwrapd.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Backend service the frontend talks to.
    pub backend: BackendServiceConfig,

    /// Usernames allowed to execute wrappers in this instance.
    /// Empty = allow all local users.
    #[serde(default)]
    pub allowed_users: Vec<String>,

    /// Reusable credentials, keyed by credential reference.
    #[serde(default)]
    pub credentials: HashMap<String, CredentialDef>,

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

/// A reusable credential definition, keyed by credential reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialDef {
    #[serde(flatten)]
    pub mode: CredentialMode,
}

/// How a credential is exposed to the wrapped target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", deny_unknown_fields)]
pub enum CredentialMode {
    /// Inject as an environment variable.
    #[serde(rename = "env")]
    Env { variable: String },
    /// Expose as a private Secret Service namespace.
    #[serde(rename = "libsecret")]
    Libsecret {},
    /// Expose as a private directory mounted into the target home view.
    #[serde(rename = "directory")]
    Directory { path: String },
}

/// A wrapper definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrapperDef {
    /// Absolute path to the target binary to exec.
    pub target: PathBuf,

    /// Credential usage bindings for this wrapper, keyed by credential ref.
    #[serde(rename = "use", default)]
    pub use_map: HashMap<String, CredentialUse>,

    /// Unix socket path for this wrapper's authenticated daemon channel.
    pub socket: PathBuf,

    /// Unix group name for GID-based wrapper identity.
    pub group: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialUse {
    pub access: AccessMode,
}

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
        assert!(config.credentials.is_empty());
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
credentials:
  github/token:
    mode: env
    variable: GH_TOKEN
wrappers:
  gh-authorized:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-authorized.sock
    group: underwrap-gh-authorized
    use:
      github/token:
        access: read-only
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.wrappers.len(), 1);
        assert_eq!(config.credentials.len(), 1);
        let w = &config.wrappers["gh-authorized"];
        assert_eq!(w.target, PathBuf::from("/usr/bin/gh"));
        assert_eq!(w.use_map.len(), 1);
        match &config.credentials["github/token"].mode {
            CredentialMode::Env { variable } => assert_eq!(variable, "GH_TOKEN"),
            other => panic!("expected env credential, got {other:?}"),
        }
    }

    #[test]
    fn directory_credential_parses() {
        let yaml = r#"
backend:
  url: https://backend.example.internal
  ca_file: /x
  client_cert_file: /y
  client_key_file: /z
credentials:
  gws/state:
    mode: directory
    path: .config/gws
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        match &config.credentials["gws/state"].mode {
            CredentialMode::Directory { path } => assert_eq!(path, ".config/gws"),
            other => panic!("expected directory credential, got {other:?}"),
        }
    }

    #[test]
    fn directory_credential_requires_path() {
        let yaml = r#"
backend:
  url: https://x
  ca_file: /x
  client_cert_file: /y
  client_key_file: /z
credentials:
  gws/state:
    mode: directory
"#;
        let err = serde_yaml::from_str::<DaemonConfig>(yaml).unwrap_err();
        assert!(err.to_string().contains("missing field `path`"));
    }

    #[test]
    fn env_credential_rejects_path_field() {
        let yaml = r#"
backend:
  url: https://x
  ca_file: /x
  client_cert_file: /y
  client_key_file: /z
credentials:
  jira/api-token:
    mode: env
    variable: JIRA_API_TOKEN
    path: .config/jira
"#;
        let err = serde_yaml::from_str::<DaemonConfig>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown field `path`"));
    }

    #[test]
    fn directory_credential_rejects_variable_field() {
        let yaml = r#"
backend:
  url: https://x
  ca_file: /x
  client_cert_file: /y
  client_key_file: /z
credentials:
  gws/state:
    mode: directory
    path: .config/gws
    variable: SHOULD_NOT_EXIST
"#;
        let err = serde_yaml::from_str::<DaemonConfig>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown field `variable`"));
    }

    #[test]
    fn libsecret_credential_rejects_path_field() {
        let yaml = r#"
backend:
  url: https://x
  ca_file: /x
  client_cert_file: /y
  client_key_file: /z
credentials:
  github/gh:
    mode: libsecret
    path: .config/gh
"#;
        let err = serde_yaml::from_str::<DaemonConfig>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown field `path`"));
    }
}
