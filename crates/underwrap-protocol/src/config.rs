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
}

impl Default for SocketConfig {
    fn default() -> Self {
        Self {
            read: default_read_socket(),
            control: default_control_socket(),
        }
    }
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
    // Future: OAuthRefreshToAccessToken, etc.
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
