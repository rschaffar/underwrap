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

    /// Secret storage backend.
    #[serde(default)]
    pub storage: StorageConfig,

    /// Wrapper definitions, keyed by wrapper name.
    #[serde(default)]
    pub wrappers: HashMap<String, WrapperDef>,

    /// Secret definitions, keyed by secret id (e.g. "github/token").
    #[serde(default)]
    pub secrets: HashMap<String, SecretDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketConfig {
    /// Path for the read socket.
    #[serde(default = "default_read_socket")]
    pub read: PathBuf,

    /// Path for the control socket.
    #[serde(default = "default_control_socket")]
    pub control: PathBuf,

    /// Unix permission mode for the read socket.
    /// Accepts integer (432) or octal string ("0660").
    #[serde(default = "default_read_mode", deserialize_with = "deserialize_mode")]
    pub read_mode: u32,

    /// Unix permission mode for the control socket.
    /// Accepts integer (384) or octal string ("0600").
    #[serde(
        default = "default_control_mode",
        deserialize_with = "deserialize_mode"
    )]
    pub control_mode: u32,

    /// Allowed UIDs for the read socket. Empty = allow all.
    #[serde(default)]
    pub read_allow_uids: Vec<u32>,

    /// Allowed GIDs for the read socket. Empty = allow all.
    #[serde(default)]
    pub read_allow_gids: Vec<u32>,

    /// Allowed UIDs for the control socket. Empty = allow all.
    #[serde(default)]
    pub control_allow_uids: Vec<u32>,

    /// Allowed GIDs for the control socket. Empty = allow all.
    #[serde(default)]
    pub control_allow_gids: Vec<u32>,

    /// Expected executable path for wrapper processes on the read socket.
    /// If set, the daemon verifies /proc/<pid>/exe of each read-socket peer
    /// matches this path. Blocks non-wrapper processes from requesting secrets.
    #[serde(default)]
    pub read_allow_exe: Option<PathBuf>,
}

impl Default for SocketConfig {
    fn default() -> Self {
        Self {
            read: default_read_socket(),
            control: default_control_socket(),
            read_mode: default_read_mode(),
            control_mode: default_control_mode(),
            read_allow_uids: Vec::new(),
            read_allow_gids: Vec::new(),
            control_allow_uids: Vec::new(),
            control_allow_gids: Vec::new(),
            read_allow_exe: None,
        }
    }
}

fn default_read_mode() -> u32 {
    0o660
}

fn default_control_mode() -> u32 {
    0o600
}

/// Deserialize a Unix permission mode from either an integer or an octal string.
/// Accepts: 432 (decimal), "0660" (octal string), "660" (octal string without prefix).
fn deserialize_mode<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct ModeVisitor;

    impl<'de> de::Visitor<'de> for ModeVisitor {
        type Value = u32;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an integer or octal string like \"0660\"")
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

fn default_read_socket() -> PathBuf {
    PathBuf::from(crate::READ_SOCKET_PATH)
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

/// A wrapper definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrapperDef {
    /// Absolute path to the target binary to exec.
    pub target: PathBuf,

    /// Secret reference (must match a key in `secrets`).
    pub secret: String,

    /// How the secret is injected into the child process.
    #[serde(default)]
    pub injection: InjectionMode,

    /// Optional: the derivation to perform on the stored secret before injection.
    #[serde(default)]
    pub derivation: DerivationMode,

    /// Whether this wrapper requires an explicit unlock before reads.
    /// If true, reads are denied unless an active unlock grant exists.
    #[serde(default)]
    pub require_unlock: bool,

    /// Unix group name for GID-based wrapper identity.
    ///
    /// When set, the daemon resolves this group to a GID at startup and
    /// uses SO_PEERCRED.gid to verify the caller's identity. The wrapper
    /// binary must be installed setgid to this group (e.g. via
    /// security.wrappers on NixOS).
    ///
    /// When unset, the daemon trusts the client-supplied wrapper name
    /// (less secure, suitable for development).
    #[serde(default)]
    pub group: Option<String>,
}

/// How the secret material is delivered to the child process.
///
/// Currently only `env` is implemented. Additional modes (stdin, tempfile)
/// may be added later.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum InjectionMode {
    /// Set an environment variable.
    #[serde(rename = "env")]
    Env {
        #[serde(default = "default_env_var")]
        variable: String,
    },
}

impl Default for InjectionMode {
    fn default() -> Self {
        Self::Env {
            variable: default_env_var(),
        }
    }
}

fn default_env_var() -> String {
    "UNDERWRAP_SECRET".to_string()
}

/// Derivation applied to stored secret before injection.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum DerivationMode {
    /// Return the stored secret as-is.
    #[default]
    #[serde(rename = "none")]
    None,
    /// Exchange an OAuth refresh token (stored as Google authorized-user JSON)
    /// for a short-lived access token. The daemon performs the token exchange
    /// and returns only the access token.
    #[serde(rename = "oauth_refresh_to_access_token")]
    OAuthRefreshToAccessToken,
}

/// A secret entry definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretDef {
    /// Whether this secret can be updated at runtime.
    #[serde(default)]
    pub mutable: bool,

    /// Labels / metadata.
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_config() {
        let yaml = "{}";
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.wrappers.is_empty());
        assert!(config.secrets.is_empty());
    }

    #[test]
    fn full_wrapper_config() {
        let yaml = r#"
wrappers:
  gh-authorized:
    target: /usr/bin/gh
    secret: github/token
    injection:
      mode: env
      variable: GH_TOKEN
secrets:
  github/token:
    mutable: false
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.wrappers.len(), 1);
        let w = &config.wrappers["gh-authorized"];
        assert_eq!(w.target, PathBuf::from("/usr/bin/gh"));
        assert_eq!(w.secret, "github/token");
        let InjectionMode::Env { variable } = &w.injection;
        assert_eq!(variable, "GH_TOKEN");
    }

    #[test]
    fn injection_defaults() {
        let yaml = r#"
wrappers:
  w:
    target: /bin/true
    secret: s
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        let w = &config.wrappers["w"];
        let InjectionMode::Env { variable } = &w.injection;
        assert_eq!(variable, "UNDERWRAP_SECRET");
    }

    #[test]
    fn mutable_secret() {
        let yaml = r#"
secrets:
  google/castlabs:
    mutable: true
    labels:
      owner: castlabs
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        let s = &config.secrets["google/castlabs"];
        assert!(s.mutable);
        assert_eq!(s.labels["owner"], "castlabs");
    }

    #[test]
    fn storage_plain_config() {
        let yaml = r#"
storage:
  backend: plain
  dir: /tmp/test-secrets
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        match &config.storage {
            StorageConfig::Plain { dir } => assert_eq!(dir, &PathBuf::from("/tmp/test-secrets")),
            _ => panic!("expected Plain backend"),
        }
    }

    #[test]
    fn config_roundtrip() {
        let config = DaemonConfig {
            sockets: SocketConfig::default(),
            storage: StorageConfig::default(),
            wrappers: {
                let mut m = HashMap::new();
                m.insert(
                    "test".to_string(),
                    WrapperDef {
                        target: PathBuf::from("/bin/test"),
                        secret: "test/secret".to_string(),
                        injection: InjectionMode::Env {
                            variable: "MY_VAR".to_string(),
                        },
                        derivation: DerivationMode::None,
                        require_unlock: false,
                        group: None,
                    },
                );
                m
            },
            secrets: {
                let mut m = HashMap::new();
                m.insert(
                    "test/secret".to_string(),
                    SecretDef {
                        mutable: true,
                        labels: HashMap::new(),
                    },
                );
                m
            },
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: DaemonConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.wrappers.len(), 1);
        assert_eq!(parsed.secrets.len(), 1);
    }
}
