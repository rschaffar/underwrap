//! Configuration schema — maps to the runtime YAML config loaded by underwrapd.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Top-level daemon configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Socket configuration.
    #[serde(default)]
    pub sockets: SocketConfig,

    /// Backend connection used by the frontend daemon.
    #[serde(default)]
    pub backend: BackendSocketConfig,

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketConfig {
    /// Path for the control socket.
    #[serde(default = "default_control_socket")]
    pub control: PathBuf,

    /// Unix permission mode for the control socket.
    /// Accepts integer (384) or octal string ("0600").
    #[serde(
        default = "default_control_mode",
        deserialize_with = "deserialize_mode"
    )]
    pub control_mode: u32,

    /// Allowed UIDs for the control socket. Empty = allow all.
    #[serde(default)]
    pub control_allow_uids: Vec<u32>,

    /// Allowed GIDs for the control socket. Empty = allow all.
    #[serde(default)]
    pub control_allow_gids: Vec<u32>,
}

impl Default for SocketConfig {
    fn default() -> Self {
        Self {
            control: default_control_socket(),
            control_mode: default_control_mode(),
            control_allow_uids: Vec::new(),
            control_allow_gids: Vec::new(),
        }
    }
}

fn default_control_mode() -> u32 {
    0o600
}

/// Deserialize a Unix permission mode from either an integer or an octal string.
/// Accepts: 384 (decimal), "0600" (octal string), "600" (octal string without prefix).
fn deserialize_mode<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct ModeVisitor;

    impl<'de> de::Visitor<'de> for ModeVisitor {
        type Value = u32;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an integer or octal string like \"0600\"")
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<u32, E> {
            u32::try_from(v).map_err(|_| E::custom(format!("mode {v} out of range")))
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<u32, E> {
            u32::try_from(v).map_err(|_| E::custom(format!("mode {v} out of range")))
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<u32, E> {
            let s = v
                .strip_prefix("0o")
                .or_else(|| v.strip_prefix("0"))
                .unwrap_or(v);
            u32::from_str_radix(s, 8).map_err(|_| E::custom(format!("invalid octal mode: {v:?}")))
        }
    }

    deserializer.deserialize_any(ModeVisitor)
}

fn default_control_socket() -> PathBuf {
    PathBuf::from(crate::CONTROL_SOCKET_PATH)
}

/// Storage backend configuration.
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
    /// Remote backend: connects to underwrap-backendd over a Unix socket.
    /// The socket is typically SSH-tunneled from a workstation.
    #[serde(rename = "remote")]
    Remote {
        /// Path to the backend Unix socket.
        socket: PathBuf,
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
    pub fn dir(&self) -> Option<&PathBuf> {
        match self {
            Self::Plain { dir } | Self::Encrypted { dir, .. } => Some(dir),
            Self::Remote { .. } => None,
        }
    }
}

fn default_storage_dir() -> PathBuf {
    PathBuf::from("/var/lib/underwrap/secrets")
}

/// Backend transport mode used by `underwrapd`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum BackendMode {
    /// Connect to a backend protocol over a Unix socket.
    #[default]
    #[serde(rename = "unix-socket")]
    UnixSocket,
    /// Connect to a backend service over HTTPS.
    #[serde(rename = "service")]
    Service,
}

/// Frontend backend-client configuration.
///
/// The historical shape was just a Unix socket path. The struct now carries a
/// transport mode so the frontend can evolve toward a service backend without
/// changing the rest of the daemon architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendSocketConfig {
    /// Backend transport mode.
    #[serde(default)]
    pub mode: BackendMode,

    /// Path to the backend Unix socket when `mode = "unix-socket"`.
    #[serde(default = "default_backend_socket")]
    pub socket: PathBuf,

    /// Backend service URL when `mode = "service"`.
    #[serde(default)]
    pub url: Option<String>,

    /// CA certificate bundle used to verify the backend service when
    /// `mode = "service"`.
    #[serde(default)]
    pub ca_file: Option<PathBuf>,

    /// Client certificate file used for mTLS when `mode = "service"`.
    #[serde(default)]
    pub client_cert_file: Option<PathBuf>,

    /// Client private key file used for mTLS when `mode = "service"`.
    #[serde(default)]
    pub client_key_file: Option<PathBuf>,
}

impl Default for BackendSocketConfig {
    fn default() -> Self {
        Self {
            mode: BackendMode::UnixSocket,
            socket: default_backend_socket(),
            url: None,
            ca_file: None,
            client_cert_file: None,
            client_key_file: None,
        }
    }
}

fn default_backend_socket() -> PathBuf {
    PathBuf::from("/run/underwrap/backend.sock")
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

    /// Wrapper unlock policy.
    #[serde(default)]
    pub unlock: UnlockPolicy,

    /// Unix socket path for this wrapper's authenticated daemon channel.
    pub socket: PathBuf,

    /// Unix group name for GID-based wrapper identity.
    pub group: String,

    /// SSH public key (OpenSSH format) required for unlock authentication.
    /// If set, unlock requests can use SSH-agent challenge flow.
    #[serde(default)]
    pub unlock_key: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnlockPolicy {
    #[serde(default)]
    pub mode: UnlockMode,
    #[serde(rename = "maxSeconds")]
    pub max_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum UnlockMode {
    #[default]
    #[serde(rename = "always-unlocked")]
    AlwaysUnlocked,
    #[serde(rename = "always")]
    Always,
    #[serde(rename = "optional")]
    Optional,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_config() {
        let yaml = "{}";
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.wrappers.is_empty());
        assert!(config.credentials.is_empty());
        assert_eq!(config.backend.mode, BackendMode::UnixSocket);
        assert_eq!(
            config.backend.socket,
            PathBuf::from("/run/underwrap/backend.sock")
        );
        assert_eq!(config.backend.url, None);
        assert_eq!(config.backend.ca_file, None);
        assert_eq!(config.backend.client_cert_file, None);
        assert_eq!(config.backend.client_key_file, None);
    }

    #[test]
    fn full_wrapper_config() {
        let yaml = r#"
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
    unlock:
      mode: always-unlocked
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.wrappers.len(), 1);
        assert_eq!(config.credentials.len(), 1);
        let w = &config.wrappers["gh-authorized"];
        assert_eq!(w.target, PathBuf::from("/usr/bin/gh"));
        assert_eq!(w.use_map.len(), 1);
        assert_eq!(w.unlock.mode, UnlockMode::AlwaysUnlocked);
        match &config.credentials["github/token"].mode {
            CredentialMode::Env { variable } => assert_eq!(variable, "GH_TOKEN"),
            other => panic!("expected env credential, got {other:?}"),
        }
    }

    #[test]
    fn unlock_policy_parses() {
        let yaml = r#"
credentials:
  github/gh:
    mode: libsecret
wrappers:
  gh-auth:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-auth.sock
    group: underwrap-gh-auth
    use:
      github/gh:
        access: read-write
    unlock:
      mode: optional
      maxSeconds: 1800
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        let w = &config.wrappers["gh-auth"];
        assert_eq!(w.unlock.mode, UnlockMode::Optional);
        assert_eq!(w.unlock.max_seconds, Some(1800));
        assert_eq!(w.use_map["github/gh"].access, AccessMode::ReadWrite);
    }

    #[test]
    fn backend_socket_config() {
        let yaml = r#"
backend:
  socket: /tmp/test-backend.sock
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.backend.mode, BackendMode::UnixSocket);
        assert_eq!(
            config.backend.socket,
            PathBuf::from("/tmp/test-backend.sock")
        );
        assert_eq!(config.backend.url, None);
        assert_eq!(config.backend.ca_file, None);
        assert_eq!(config.backend.client_cert_file, None);
        assert_eq!(config.backend.client_key_file, None);
    }

    #[test]
    fn backend_service_config() {
        let yaml = r#"
backend:
  mode: service
  url: https://backend.example.internal:8443
  ca_file: /etc/underwrap/ca.pem
  client_cert_file: /etc/underwrap/client.pem
  client_key_file: /etc/underwrap/client.key
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.backend.mode, BackendMode::Service);
        assert_eq!(
            config.backend.url.as_deref(),
            Some("https://backend.example.internal:8443")
        );
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
    fn directory_credential_parses() {
        let yaml = r#"
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
credentials:
  github/gh:
    mode: libsecret
    path: .config/gh
"#;
        let err = serde_yaml::from_str::<DaemonConfig>(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown field `path`"));
    }

    #[test]
    fn config_roundtrip() {
        let config = DaemonConfig {
            sockets: SocketConfig::default(),
            backend: BackendSocketConfig {
                mode: BackendMode::UnixSocket,
                socket: PathBuf::from("/run/underwrap/backend.sock"),
                url: None,
                ca_file: None,
                client_cert_file: None,
                client_key_file: None,
            },
            allowed_users: vec!["alice".to_string()],
            credentials: {
                let mut m = HashMap::new();
                m.insert(
                    "test/secret".to_string(),
                    CredentialDef {
                        mode: CredentialMode::Env {
                            variable: "MY_VAR".to_string(),
                        },
                    },
                );
                m
            },
            wrappers: {
                let mut m = HashMap::new();
                m.insert(
                    "test".to_string(),
                    WrapperDef {
                        target: PathBuf::from("/bin/test"),
                        use_map: {
                            let mut u = HashMap::new();
                            u.insert(
                                "test/secret".to_string(),
                                CredentialUse {
                                    access: AccessMode::ReadOnly,
                                },
                            );
                            u
                        },
                        unlock: UnlockPolicy::default(),
                        socket: PathBuf::from("/run/underwrap/test.sock"),
                        group: "testgroup".to_string(),
                        unlock_key: None,
                    },
                );
                m
            },
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: DaemonConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.backend.mode, BackendMode::UnixSocket);
        assert_eq!(
            parsed.backend.socket,
            PathBuf::from("/run/underwrap/backend.sock")
        );
        assert_eq!(parsed.backend.url, None);
        assert_eq!(parsed.backend.ca_file, None);
        assert_eq!(parsed.backend.client_cert_file, None);
        assert_eq!(parsed.backend.client_key_file, None);
        assert_eq!(parsed.wrappers.len(), 1);
        assert_eq!(parsed.credentials.len(), 1);
    }
}
