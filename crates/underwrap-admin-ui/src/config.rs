//! Admin UI configuration schema.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Top-level configuration loaded from YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUiConfig {
    /// Bind address, e.g. `127.0.0.1:8080`.
    pub listen: String,

    /// Path to a file containing the 32-byte random session key used to sign
    /// session cookies. Generated once by the operator; must stay stable
    /// across UI restarts to keep sessions valid.
    pub session_key_file: PathBuf,

    /// Operator accounts keyed by username. Each entry is read from a file
    /// containing an Argon2 PHC hash.
    pub operators: HashMap<String, OperatorConfig>,

    /// Backend targets keyed by short name. The operator picks a target
    /// after logging in.
    pub backends: HashMap<String, BackendTargetConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorConfig {
    /// File containing the Argon2 PHC-encoded password hash.
    pub password_hash_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendTargetConfig {
    /// Backend admin API URL, e.g. `http://backend.internal:19443`.
    pub admin_url: String,
    /// File containing the bearer token used to authenticate admin requests.
    pub token_file: PathBuf,
}

/// Loaded, validated runtime representation of the config.
#[derive(Debug)]
pub struct LoadedConfig {
    pub listen: String,
    pub session_key: [u8; 32],
    pub operators: HashMap<String, String>, // username → argon2 PHC hash
    pub backends: HashMap<String, LoadedBackendTarget>,
}

#[derive(Debug)]
pub struct LoadedBackendTarget {
    pub admin_url: String,
    pub token: String,
}

impl LoadedConfig {
    pub fn load(config: &AdminUiConfig) -> Result<Self> {
        anyhow::ensure!(
            !config.operators.is_empty(),
            "admin UI config must define at least one operator"
        );
        anyhow::ensure!(
            !config.backends.is_empty(),
            "admin UI config must define at least one backend target"
        );

        let session_key = load_session_key(&config.session_key_file)?;

        let mut operators = HashMap::new();
        for (name, op) in &config.operators {
            let raw = std::fs::read_to_string(&op.password_hash_file).with_context(|| {
                format!(
                    "reading password hash file for operator '{name}' at {}",
                    op.password_hash_file.display()
                )
            })?;
            let hash = raw.trim().to_string();
            anyhow::ensure!(
                !hash.is_empty(),
                "password hash file for operator '{name}' at {} is empty",
                op.password_hash_file.display()
            );
            anyhow::ensure!(
                hash.starts_with("$argon2"),
                "password hash for operator '{name}' is not a PHC-encoded argon2 hash"
            );
            operators.insert(name.clone(), hash);
        }

        let mut backends = HashMap::new();
        for (name, target) in &config.backends {
            let raw = std::fs::read_to_string(&target.token_file).with_context(|| {
                format!(
                    "reading admin token for backend '{name}' at {}",
                    target.token_file.display()
                )
            })?;
            let token = raw.trim().to_string();
            anyhow::ensure!(
                !token.is_empty(),
                "admin token for backend '{name}' at {} is empty",
                target.token_file.display()
            );
            backends.insert(
                name.clone(),
                LoadedBackendTarget {
                    admin_url: target.admin_url.clone(),
                    token,
                },
            );
        }

        Ok(Self {
            listen: config.listen.clone(),
            session_key,
            operators,
            backends,
        })
    }
}

fn load_session_key(path: &Path) -> Result<[u8; 32]> {
    let raw = std::fs::read(path)
        .with_context(|| format!("reading session key file {}", path.display()))?;
    let bytes = if raw.len() >= 32 {
        raw[..32].to_vec()
    } else {
        return Err(anyhow!(
            "session key file {} must contain at least 32 bytes (got {})",
            path.display(),
            raw.len()
        ));
    };
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_config_without_any_operator() {
        let dir = tempfile::TempDir::new().unwrap();
        let key = dir.path().join("key");
        std::fs::write(&key, [0u8; 32]).unwrap();
        let config = AdminUiConfig {
            listen: "127.0.0.1:0".to_string(),
            session_key_file: key,
            operators: HashMap::new(),
            backends: HashMap::from([(
                "b".to_string(),
                BackendTargetConfig {
                    admin_url: "http://x".to_string(),
                    token_file: dir.path().join("t"),
                },
            )]),
        };
        let err = LoadedConfig::load(&config).unwrap_err();
        assert!(err.to_string().contains("operator"));
    }

    #[test]
    fn rejects_non_argon2_hash() {
        let dir = tempfile::TempDir::new().unwrap();
        let key = dir.path().join("key");
        std::fs::write(&key, [0u8; 32]).unwrap();
        let hash_path = dir.path().join("h");
        std::fs::write(&hash_path, "plaintext").unwrap();
        let token_path = dir.path().join("t");
        std::fs::write(&token_path, "abc").unwrap();

        let config = AdminUiConfig {
            listen: "127.0.0.1:0".to_string(),
            session_key_file: key,
            operators: HashMap::from([(
                "alice".to_string(),
                OperatorConfig {
                    password_hash_file: hash_path,
                },
            )]),
            backends: HashMap::from([(
                "b".to_string(),
                BackendTargetConfig {
                    admin_url: "http://x".to_string(),
                    token_file: token_path,
                },
            )]),
        };
        let err = LoadedConfig::load(&config).unwrap_err();
        assert!(err.to_string().contains("argon2"));
    }

    #[test]
    fn loads_valid_config_and_trims_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let key = dir.path().join("key");
        std::fs::write(&key, [7u8; 64]).unwrap();

        let hash_path = dir.path().join("h");
        // A valid PHC prefix; we only check shape at load time, not password.
        std::fs::write(&hash_path, "$argon2id$v=19$m=19456,t=2,p=1$abc$abc\n").unwrap();

        let token_path = dir.path().join("t");
        std::fs::write(&token_path, "  the-token  \n").unwrap();

        let config = AdminUiConfig {
            listen: "127.0.0.1:0".to_string(),
            session_key_file: key,
            operators: HashMap::from([(
                "alice".to_string(),
                OperatorConfig {
                    password_hash_file: hash_path,
                },
            )]),
            backends: HashMap::from([(
                "b".to_string(),
                BackendTargetConfig {
                    admin_url: "http://x".to_string(),
                    token_file: token_path,
                },
            )]),
        };
        let loaded = LoadedConfig::load(&config).unwrap();
        assert_eq!(loaded.session_key.len(), 32);
        assert_eq!(loaded.session_key, [7u8; 32]);
        assert_eq!(loaded.backends["b"].token, "the-token");
        assert!(loaded.operators["alice"].starts_with("$argon2id$"));
    }
}
