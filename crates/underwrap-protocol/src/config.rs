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

    /// Unix permission mode for the read socket (octal, e.g. 0o660).
    #[serde(default = "default_read_mode")]
    pub read_mode: u32,

    /// Unix permission mode for the control socket (octal, e.g. 0o600).
    #[serde(default = "default_control_mode")]
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
        }
    }
}

fn default_read_mode() -> u32 {
    0o660
}

fn default_control_mode() -> u32 {
    0o600
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
    // Future: encrypted, hardware-assisted, etc.
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self::Plain {
            dir: default_storage_dir(),
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
}

/// How the secret material is delivered to the child process.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode")]
pub enum InjectionMode {
    /// Set an environment variable.
    #[serde(rename = "env")]
    Env {
        #[serde(default = "default_env_var")]
        variable: String,
    },
    /// Pipe to child's stdin.
    #[serde(rename = "stdin")]
    Stdin,
    /// Write to a temporary file and pass path via env var.
    #[serde(rename = "tempfile")]
    TempFile {
        #[serde(default = "default_env_var")]
        path_variable: String,
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
        match &w.injection {
            InjectionMode::Env { variable } => assert_eq!(variable, "GH_TOKEN"),
            other => panic!("expected Env, got {other:?}"),
        }
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
        match &w.injection {
            InjectionMode::Env { variable } => assert_eq!(variable, "UNDERWRAP_SECRET"),
            other => panic!("expected default Env, got {other:?}"),
        }
    }

    #[test]
    fn stdin_injection() {
        let yaml = r#"
wrappers:
  w:
    target: /bin/true
    secret: s
    injection:
      mode: stdin
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(matches!(
            config.wrappers["w"].injection,
            InjectionMode::Stdin
        ));
    }

    #[test]
    fn tempfile_injection() {
        let yaml = r#"
wrappers:
  w:
    target: /bin/true
    secret: s
    injection:
      mode: tempfile
      path_variable: MY_PATH
"#;
        let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
        match &config.wrappers["w"].injection {
            InjectionMode::TempFile { path_variable } => assert_eq!(path_variable, "MY_PATH"),
            other => panic!("expected TempFile, got {other:?}"),
        }
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
