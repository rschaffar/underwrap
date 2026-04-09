//! End-to-end integration tests.

use std::collections::HashMap;

use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::*;

use crate::harness::*;

// ---------------------------------------------------------------------------
// Utility: binary lookup (must come before tests that use require_bin!)
// ---------------------------------------------------------------------------

/// Find the path to a cargo-built binary.
/// Returns None if not found (allows tests to skip gracefully).
fn try_cargo_bin(name: &str) -> Option<String> {
    let env_key = format!("CARGO_BIN_EXE_{name}");
    if let Ok(p) = std::env::var(&env_key)
        && std::path::Path::new(&p).exists()
    {
        return Some(p);
    }

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    for profile in &["debug", "release"] {
        let path = workspace_root.join("target").join(profile).join(name);
        if path.exists() {
            return Some(path.to_str().unwrap().to_string());
        }
    }

    // Search PATH as fallback.
    if let Ok(output) = std::process::Command::new("which").arg(name).output()
        && output.status.success()
    {
        return Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    None
}

/// Find the path to a cargo-built binary, panicking if not found.
#[allow(dead_code)]
fn cargo_bin(name: &str) -> String {
    try_cargo_bin(name).unwrap_or_else(|| {
        panic!(
            "binary '{name}' not found. Run `cargo build` first. \
             Searched: CARGO_BIN_EXE_{name}, target/{{debug,release}}/{name}, PATH",
        )
    })
}

/// Skip a test if required binaries aren't available (e.g. in nix sandbox).
macro_rules! require_bin {
    ($name:expr) => {
        match try_cargo_bin($name) {
            Some(p) => p,
            None => {
                panic!("required binary '{}' not available", $name);
            }
        }
    };
}

/// Find the `env` binary (absolute path). Returns None if not found.
/// Find the `echo` binary (absolute path). Returns None if not found.
fn which_echo() -> Option<String> {
    find_bin("echo")
}

/// Search common paths for a binary (NixOS-compatible).
/// Always returns an absolute path or None.
fn find_bin(name: &str) -> Option<String> {
    // Try which first.
    if let Ok(output) = std::process::Command::new("which").arg(name).output()
        && output.status.success()
    {
        let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if p.starts_with('/') {
            return Some(p);
        }
    }
    // Try common paths.
    for prefix in &["/usr/bin", "/bin", "/run/current-system/sw/bin"] {
        let p = format!("{prefix}/{name}");
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    // Try PATH entries directly (works in nix sandbox where 'which' may not exist).
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            let p = format!("{dir}/{name}");
            if std::path::Path::new(&p).exists() {
                return Some(p);
            }
        }
    }
    None
}

macro_rules! require_echo {
    () => {
        match which_echo() {
            Some(p) => p,
            None => {
                panic!("required binary 'echo' not found");
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn one_wrapper_config() -> (
    HashMap<String, underwrap_protocol::config::WrapperDef>,
    HashMap<String, underwrap_protocol::config::CredentialDef>,
) {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "gh-authorized".to_string(),
        wrapper("/usr/bin/gh", "github/token", "GH_TOKEN"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("github/token".to_string(), secret(false));
    (wrappers, secrets)
}

fn expect_single_env_bytes(resp: WrapperResponse) -> Vec<u8> {
    match resp {
        WrapperResponse::ReadOk { env, .. } => {
            assert_eq!(env.len(), 1, "expected exactly one env binding");
            let (_key, value) = env.into_iter().next().unwrap();
            Vec::from(value)
        }
        other => panic!("expected ReadOk, got {other:?}"),
    }
}

fn expect_single_env_string(resp: WrapperResponse) -> String {
    String::from_utf8(expect_single_env_bytes(resp)).unwrap()
}

fn expect_status_all(resp: ControlResponse) -> (Vec<String>, Vec<String>) {
    match resp {
        ControlResponse::StatusAllOk {
            wrappers,
            credentials,
        } => (wrappers, credentials),
        other => panic!("expected StatusAllOk, got {other:?}"),
    }
}

fn expect_wrapper_status(resp: ControlResponse) -> WrapperStatus {
    match resp {
        ControlResponse::StatusWrapperOk { status } => status,
        other => panic!("expected StatusWrapperOk, got {other:?}"),
    }
}

fn expect_credential_status(resp: ControlResponse) -> CredentialStatus {
    match resp {
        ControlResponse::StatusCredentialOk { status } => status,
        other => panic!("expected StatusCredentialOk, got {other:?}"),
    }
}

fn directory_credential(path: &str) -> underwrap_protocol::config::CredentialDef {
    underwrap_protocol::config::CredentialDef {
        mode: underwrap_protocol::config::CredentialMode::Directory {
            path: path.to_string(),
        },
    }
}

async fn daemon_config_error(
    credentials: HashMap<String, underwrap_protocol::config::CredentialDef>,
    wrapper_def: underwrap_protocol::config::WrapperDef,
) -> String {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let control_socket = base.join("control.sock");
    let storage_dir = base.join("secrets");

    let config = underwrap_protocol::config::DaemonConfig {
        sockets: underwrap_protocol::config::SocketConfig {
            control: control_socket,
            ..Default::default()
        },
        storage: underwrap_protocol::config::StorageConfig::Plain { dir: storage_dir },
        allowed_users: Vec::new(),
        credentials,
        wrappers: HashMap::from([("dir-wrapper".to_string(), wrapper_def)]),
    };

    underwrapd::server::run(config, None)
        .await
        .unwrap_err()
        .to_string()
}

// ===========================================================================
// Store and read flow
// ===========================================================================

mod cli;
mod daemon;
mod directory_and_status;
mod store_read;
mod unlock;
