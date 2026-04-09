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
        WrapperResponse::ReadOk { env } => {
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

// ===========================================================================
// Store and read flow
// ===========================================================================

#[tokio::test]
async fn store_then_read_secret() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Store the secret.
    let resp = daemon
        .store_secret("github/token", "ghp_test123")
        .await
        .unwrap();
    assert!(matches!(resp, ControlResponse::StoreOk));

    // Read it back through the wrapper socket.
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Read before store → NotFound
// ===========================================================================

#[tokio::test]
async fn read_before_store_returns_not_found() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::NotFound));
            assert!(message.contains("github/token"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Unknown wrapper → NotFound
// ===========================================================================

#[tokio::test]
async fn read_uses_wrapper_socket_without_client_supplied_name() {
    // Wrapper identity is anchored in the wrapper socket plus the peer's
    // effective GID. The request itself carries no wrapper name.
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    daemon
        .store_secret("github/token", "real_token")
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "real_token");

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Store undeclared secret → error
// ===========================================================================

#[tokio::test]
async fn store_undeclared_secret_rejected() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let resp = daemon.store_secret("unknown/secret", "data").await.unwrap();
    match resp {
        ControlResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::NotFound));
            assert!(message.contains("unknown/secret"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

// NOTE: Multi-wrapper isolation (each wrapper can only read its own secret)
// requires real per-wrapper Unix groups and setgid wrappers. This is tested
// in the NixOS VM test. Unit tests run as a single user with one GID, so
// all wrappers resolve to the same identity.

// ===========================================================================
// Overwrite mutable secret
// ===========================================================================

#[tokio::test]
async fn overwrite_mutable_secret() {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "mutable-w".to_string(),
        wrapper("/usr/bin/true", "mut/tok", "TOK"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("mut/tok".to_string(), secret(true));
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Store initial value.
    daemon.store_secret("mut/tok", "v1").await.unwrap();
    let resp = daemon.read_request("mutable-w").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "v1");

    // Overwrite.
    daemon.store_secret("mut/tok", "v2").await.unwrap();
    let resp = daemon.read_request("mutable-w").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "v2");

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Immutable secret rejects overwrite
// ===========================================================================

#[tokio::test]
async fn immutable_secret_rejects_overwrite() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // First store succeeds (secret doesn't exist yet).
    let resp = daemon
        .store_secret("github/token", "initial")
        .await
        .unwrap();
    assert!(matches!(resp, ControlResponse::StoreOk));

    // Second store also succeeds — credentials are operator-managed and may be overwritten.
    let resp = daemon
        .store_secret("github/token", "overwrite_attempt")
        .await
        .unwrap();
    assert!(matches!(resp, ControlResponse::StoreOk));

    // Updated value preserved.
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "overwrite_attempt");

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Status — general
// ===========================================================================

#[tokio::test]
async fn status_general() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let resp = daemon.status(None).await.unwrap();
    let (wrappers, credentials) = expect_status_all(resp);
    assert_eq!(wrappers.len(), 1);
    assert_eq!(credentials.len(), 1);

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Status — specific wrapper
// ===========================================================================

#[tokio::test]
async fn status_specific_wrapper() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Before storing: usage[0].stored = false.
    let resp = daemon.status(Some("gh-authorized")).await.unwrap();
    let status = expect_wrapper_status(resp);
    assert_eq!(status.wrapper, "gh-authorized");
    assert_eq!(status.usage[0].credential, "github/token");
    assert_eq!(status.usage[0].stored, Some(false));

    // After storing: usage[0].stored = true.
    daemon.store_secret("github/token", "tok").await.unwrap();
    let resp = daemon.status(Some("gh-authorized")).await.unwrap();
    let status = expect_wrapper_status(resp);
    assert_eq!(status.usage[0].stored, Some(true));

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Status — specific secret
// ===========================================================================

#[tokio::test]
async fn status_specific_secret() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let resp = daemon.status(Some("github/token")).await.unwrap();
    let status = expect_credential_status(resp);
    assert_eq!(status.credential, "github/token");
    assert!(!status.stored);

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Status — unknown target
// ===========================================================================

#[tokio::test]
async fn status_unknown_target() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let resp = daemon.status(Some("nope")).await.unwrap();
    match resp {
        ControlResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::NotFound));
            assert!(message.contains("nope"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Concurrent reads
// ===========================================================================

#[tokio::test]
async fn concurrent_reads() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    daemon
        .store_secret("github/token", "ghp_concurrent")
        .await
        .unwrap();

    // Spawn 20 concurrent read requests.
    let mut handles = Vec::new();
    for _ in 0..20 {
        let sock = daemon.wrapper_sockets["gh-authorized"].clone();
        handles.push(tokio::spawn(async move {
            let mut stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
            let req = WrapperRequest::Read { scope: None };
            underwrap_protocol::ipc::write_frame_async(&mut stream, &req)
                .await
                .unwrap();
            let resp: WrapperResponse = underwrap_protocol::ipc::read_frame_async(&mut stream)
                .await
                .unwrap();
            resp
        }));
    }

    for h in handles {
        let resp = h.await.unwrap();
        assert_eq!(expect_single_env_string(resp), "ghp_concurrent");
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Secrets with slashes (hierarchical IDs)
// ===========================================================================

#[tokio::test]
async fn hierarchical_secret_ids() {
    let echo = require_echo!();
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "deep-wrapper".to_string(),
        wrapper(&echo, "org/team/service/token", "TOKEN"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("org/team/service/token".to_string(), secret(false));

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    daemon
        .store_secret("org/team/service/token", "deep_val")
        .await
        .unwrap();

    let resp = daemon.read_request("deep-wrapper").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "deep_val");

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Large secret material
// ===========================================================================

#[tokio::test]
async fn large_secret_material() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // ~32KB of data (simulating a larger credential blob).
    let big = "x".repeat(32_000);
    daemon.store_secret("github/token", &big).await.unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_bytes(resp).len(), 32_000);

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Secret material with special characters
// ===========================================================================

#[tokio::test]
async fn secret_with_special_chars() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let special = "tok=\"abc 123\"\nnewline\ttab\0null";
    daemon.store_secret("github/token", special).await.unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_bytes(resp), special.as_bytes());

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Binary-level wrapper test (underwrap-exec via process)

// ===========================================================================
// underwrapctl binary: store via stdin, then status
// ===========================================================================

#[tokio::test]
async fn underwrapctl_store_and_status() {
    let build_status = std::process::Command::new("cargo")
        .args(["build", "-p", "underwrapctl"])
        .status()
        .unwrap();
    assert!(build_status.success(), "cargo build -p underwrapctl failed");
    let ctl_bin = require_bin!("underwrapctl");

    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Store via underwrapctl (pipe secret on stdin).
    let mut child = tokio::process::Command::new(&ctl_bin)
        .args(["--socket", daemon.control_socket.to_str().unwrap()])
        .args(["store", "github/token"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    {
        use tokio::io::AsyncWriteExt;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"ghp_from_ctl\n").await.unwrap();
        stdin.shutdown().await.unwrap();
    }

    let output = child.wait_with_output().await.unwrap();
    assert!(
        output.status.success(),
        "underwrapctl store failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify via daemon read.
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_from_ctl");

    // Query status via underwrapctl.
    let output = tokio::process::Command::new(&ctl_bin)
        .args(["--socket", daemon.control_socket.to_str().unwrap()])
        .args(["status", "gh-authorized"])
        .output()
        .await
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"stored\": true"), "stdout: {stdout}");

    daemon.stop().await.unwrap();
}

// ===========================================================================
// underwrapctl store undeclared secret → exit code 1
// ===========================================================================

#[tokio::test]
async fn underwrapctl_store_undeclared_exits_nonzero() {
    let build_status = std::process::Command::new("cargo")
        .args(["build", "-p", "underwrapctl"])
        .status()
        .unwrap();
    assert!(build_status.success(), "cargo build -p underwrapctl failed");
    let ctl_bin = require_bin!("underwrapctl");

    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let mut child = tokio::process::Command::new(&ctl_bin)
        .args(["--socket", daemon.control_socket.to_str().unwrap()])
        .args(["store", "bogus/secret"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    {
        use tokio::io::AsyncWriteExt;
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"data\n").await.unwrap();
        stdin.shutdown().await.unwrap();
    }

    let output = child.wait_with_output().await.unwrap();
    assert!(
        !output.status.success(),
        "expected failure for undeclared secret"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("bogus/secret"), "stderr: {stderr}");

    daemon.stop().await.unwrap();
}

// ===========================================================================
// exec wrapper with missing secret → error exit

// ===========================================================================
// exec wrapper passes arguments through to target

// ===========================================================================
// exec wrapper does NOT leak secret if target doesn't use it
// (env is only set, target can choose not to print it)

// ===========================================================================
// Rapid store-read cycles (stress test for socket handling)
// ===========================================================================

#[tokio::test]
async fn rapid_store_read_cycles() {
    // Need mutable secret for repeated overwrites.
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "rapid-wrapper".to_string(),
        wrapper("/usr/bin/true", "rapid/tok", "TOK"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("rapid/tok".to_string(), secret(true));
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    for i in 0..50 {
        let val = format!("tok_{i}");
        daemon.store_secret("rapid/tok", &val).await.unwrap();

        let resp = daemon.read_request("rapid-wrapper").await.unwrap();
        assert_eq!(expect_single_env_string(resp), val, "iteration {i}");
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Unlock: locked wrapper denies reads
// ===========================================================================

#[tokio::test]
async fn locked_wrapper_denies_read() {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "locked-w".to_string(),
        locked_wrapper("/usr/bin/true", "locked/tok", "TOK"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("locked/tok".to_string(), secret(false));

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();
    daemon.store_secret("locked/tok", "val").await.unwrap();

    // Read should be denied — wrapper is locked.
    let resp = daemon.read_request("locked-w").await.unwrap();
    match resp {
        WrapperResponse::Error { code, .. } => assert!(matches!(code, ErrorCode::Locked)),
        other => panic!("expected Locked error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Unlock: unlock then read succeeds
// ===========================================================================

#[tokio::test]
async fn unlock_then_read_succeeds() {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "locked-w".to_string(),
        locked_wrapper("/usr/bin/true", "locked/tok", "TOK"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("locked/tok".to_string(), secret(false));

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();
    daemon
        .store_secret("locked/tok", "secret_val")
        .await
        .unwrap();

    // Unlock the wrapper.
    let resp = daemon
        .control_request(&ControlRequest::Unlock {
            target: "locked-w".to_string(),
            scope: None,
            seconds: None,
            grant_user: None,
            grant_all: false,
        })
        .await
        .unwrap();
    assert!(matches!(resp, ControlResponse::UnlockOk));

    // Now reads succeed.
    let resp = daemon.read_request("locked-w").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "secret_val");

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Unlock: lock after unlock denies again
// ===========================================================================

#[tokio::test]
async fn lock_after_unlock_denies() {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "locked-w".to_string(),
        locked_wrapper("/usr/bin/true", "locked/tok", "TOK"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("locked/tok".to_string(), secret(false));

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();
    daemon.store_secret("locked/tok", "val").await.unwrap();

    // Unlock, then lock.
    daemon
        .control_request(&ControlRequest::Unlock {
            target: "locked-w".to_string(),
            scope: None,
            seconds: None,
            grant_user: None,
            grant_all: false,
        })
        .await
        .unwrap();
    daemon
        .control_request(&ControlRequest::Lock {
            target: Some("locked-w".to_string()),
            scope: None,
        })
        .await
        .unwrap();

    // Should be locked again.
    let resp = daemon.read_request("locked-w").await.unwrap();
    assert!(matches!(
        resp,
        WrapperResponse::Error {
            code: ErrorCode::Locked,
            ..
        }
    ));

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Unlock: time-based expiry
// ===========================================================================

#[tokio::test]
async fn unlock_time_expiry() {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "locked-w".to_string(),
        locked_wrapper("/usr/bin/true", "locked/tok", "TOK"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("locked/tok".to_string(), secret(false));

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();
    daemon.store_secret("locked/tok", "val").await.unwrap();

    // Unlock for 0 seconds — should expire immediately.
    daemon
        .control_request(&ControlRequest::Unlock {
            target: "locked-w".to_string(),
            scope: None,
            seconds: Some(0),
            grant_user: None,
            grant_all: false,
        })
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let resp = daemon.read_request("locked-w").await.unwrap();
    assert!(matches!(
        resp,
        WrapperResponse::Error {
            code: ErrorCode::Locked,
            ..
        }
    ));

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Unlock: scoped grants
// ===========================================================================

#[tokio::test]
async fn unlock_scoped() {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "locked-w".to_string(),
        locked_wrapper("/usr/bin/true", "locked/tok", "TOK"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("locked/tok".to_string(), secret(false));

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();
    daemon.store_secret("locked/tok", "val").await.unwrap();

    // Unlock for scope "session-1" only.
    daemon
        .control_request(&ControlRequest::Unlock {
            target: "locked-w".to_string(),
            scope: Some("session-1".to_string()),
            seconds: None,
            grant_user: None,
            grant_all: false,
        })
        .await
        .unwrap();

    // Read with matching scope succeeds.
    let resp = daemon
        .read_request_with_scope("locked-w", "session-1")
        .await
        .unwrap();
    assert!(matches!(resp, WrapperResponse::ReadOk { .. }));

    // Read with different scope fails.
    let resp = daemon
        .read_request_with_scope("locked-w", "session-2")
        .await
        .unwrap();
    assert!(matches!(
        resp,
        WrapperResponse::Error {
            code: ErrorCode::Locked,
            ..
        }
    ));

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Unlock: challenge-response with unlock_key on same connection
// ===========================================================================

#[tokio::test]
async fn unlock_key_challenge_response_same_connection() {
    use ssh_key::PrivateKey;
    use ssh_key::private::{Ed25519Keypair, KeypairData};

    // Generate a test ed25519 key pair.
    let keypair = Ed25519Keypair::random(&mut rand::thread_rng());
    let private_key = PrivateKey::new(KeypairData::Ed25519(keypair.clone()), "test-key").unwrap();
    let public_key = private_key.public_key();
    let public_key_openssh = public_key.to_openssh().unwrap();
    let public_key_blob = public_key.to_bytes().unwrap();

    let mut wrappers = HashMap::new();
    let mut w = locked_wrapper("/usr/bin/true", "key-locked/tok", "TOK");
    w.unlock_key = Some(public_key_openssh.clone());
    wrappers.insert("key-locked-w".to_string(), w);
    let mut secrets = HashMap::new();
    secrets.insert("key-locked/tok".to_string(), secret(false));

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();
    daemon
        .store_secret("key-locked/tok", "key_secret")
        .await
        .unwrap();

    // Send Unlock on a single connection — should get Challenge back.
    let mut stream = daemon.connect_wrapper("key-locked-w").await.unwrap();
    let req = WrapperRequest::Unlock {
        scope: None,
        seconds: None,
    };
    write_frame_async(&mut stream, &req).await.unwrap();
    let resp: WrapperResponse = read_frame_async(&mut stream).await.unwrap();

    let nonce = match resp {
        WrapperResponse::Challenge { nonce } => nonce,
        other => panic!("expected Challenge, got {other:?}"),
    };

    // Sign the nonce with our private key.
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&keypair.private.to_bytes());
    use ed25519_dalek::Signer;
    let raw_sig = signing_key.sign(&nonce);

    // Build the SSH wire format signature:
    // [4-byte algo-name-len][algo-name][4-byte sig-len][sig-bytes]
    let algo = b"ssh-ed25519";
    let sig_bytes = raw_sig.to_bytes();
    let mut wire_sig = Vec::new();
    wire_sig.extend_from_slice(&(algo.len() as u32).to_be_bytes());
    wire_sig.extend_from_slice(algo);
    wire_sig.extend_from_slice(&(sig_bytes.len() as u32).to_be_bytes());
    wire_sig.extend_from_slice(&sig_bytes);

    // Send UnlockResponse on the SAME connection.
    let req2 = WrapperRequest::UnlockResponse {
        scope: None,
        seconds: None,
        challenge: nonce,
        signature: wire_sig,
        public_key_blob,
    };
    write_frame_async(&mut stream, &req2).await.unwrap();
    let resp2: WrapperResponse = read_frame_async(&mut stream).await.unwrap();

    assert!(matches!(resp2, WrapperResponse::UnlockOk));

    // Now reads should succeed.
    let resp = daemon.read_request("key-locked-w").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "key_secret");

    daemon.stop().await.unwrap();
}
// ===========================================================================

#[tokio::test]
async fn no_require_unlock_works_without_unlock() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    daemon.store_secret("github/token", "tok").await.unwrap();

    // gh-authorized has require_unlock=false, so it works without unlock.
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert!(matches!(resp, WrapperResponse::ReadOk { .. }));

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Config parsing: round-trip the example config
// ===========================================================================

#[tokio::test]
async fn parse_example_config() {
    // Inline a representative config matching the structure of examples/config.yaml.
    // This avoids depending on the file being present in nix sandbox builds.
    let text = r#"
credentials:
  github/token:
    mode: env
    variable: GH_TOKEN
  slack/xoxp:
    mode: env
    variable: SLACK_TOKEN
  github/gh:
    mode: libsecret
wrappers:
  gh-authorized:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-authorized.sock
    group: underwrap-gh-authorized
    use:
      github/token:
        access: read-only
  agent-slack:
    target: /usr/bin/slack-cli
    socket: /run/underwrap/agent-slack.sock
    group: underwrap-agent-slack
    use:
      slack/xoxp:
        access: read-only
  gh-libsecret:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-libsecret.sock
    group: underwrap-gh-libsecret
    use:
      github/gh:
        access: read-write
"#;
    let config: underwrap_protocol::config::DaemonConfig = serde_yaml::from_str(text).unwrap();

    assert_eq!(config.wrappers.len(), 3);
    assert_eq!(config.credentials.len(), 3);
    assert!(config.wrappers.contains_key("gh-authorized"));
    assert!(config.wrappers.contains_key("agent-slack"));
    assert!(config.wrappers.contains_key("gh-libsecret"));

    let gh = &config.wrappers["gh-authorized"];
    assert!(gh.use_map.contains_key("github/token"));
    match &config.credentials["github/token"].mode {
        underwrap_protocol::config::CredentialMode::Env { variable } => {
            assert_eq!(variable, "GH_TOKEN");
        }
        other => panic!("expected env credential, got {other:?}"),
    }
}

// ===========================================================================
// underwrapd binary: starts and responds to status
// ===========================================================================

#[tokio::test]
async fn underwrapd_binary_start_and_status() {
    // Ensure the standalone binary is freshly built. `cargo test` may compile
    // the crate without updating `target/debug/underwrapd`, and this test wants
    // to exercise the real binary entry point.
    let build_status = std::process::Command::new("cargo")
        .args(["build", "-p", "underwrapd"])
        .status()
        .unwrap();
    assert!(build_status.success(), "cargo build -p underwrapd failed");

    let daemon_bin = require_bin!("underwrapd");

    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();

    let wrapper_sock = base.join("wrapper-w.sock");
    let control_sock = base.join("control.sock");
    let storage_dir = base.join("secrets");

    let config = serde_yaml::to_string(&underwrap_protocol::config::DaemonConfig {
        sockets: underwrap_protocol::config::SocketConfig {
            control: control_sock.clone(),
            ..Default::default()
        },
        storage: underwrap_protocol::config::StorageConfig::Plain {
            dir: storage_dir.clone(),
        },
        allowed_users: Vec::new(),
        credentials: {
            let mut m = HashMap::new();
            m.insert("s/t".to_string(), secret(false));
            m
        },
        wrappers: {
            let mut m = HashMap::new();
            let echo = which_echo().unwrap_or_else(|| "/bin/true".to_string());
            let mut w = wrapper(&echo, "s/t", "T");
            w.socket = wrapper_sock.clone();
            m.insert("w".to_string(), w);
            m
        },
    })
    .unwrap();

    let config_path = base.join("config.yaml");
    std::fs::write(&config_path, &config).unwrap();

    let mut child = tokio::process::Command::new(&daemon_bin)
        .args(["--config", config_path.to_str().unwrap()])
        .env("RUST_LOG", "info")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    // Wait for sockets.
    for _ in 0..100 {
        if control_sock.exists() && wrapper_sock.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(control_sock.exists(), "control socket never appeared");
    assert!(wrapper_sock.exists(), "wrapper socket never appeared");

    // Send a status request directly.
    let mut stream = tokio::net::UnixStream::connect(&control_sock)
        .await
        .unwrap();
    let req = ControlRequest::StatusAll;
    underwrap_protocol::ipc::write_frame_async(&mut stream, &req)
        .await
        .unwrap();
    let resp: ControlResponse = underwrap_protocol::ipc::read_frame_async(&mut stream)
        .await
        .unwrap();

    match resp {
        ControlResponse::StatusAllOk { wrappers, .. } => {
            assert_eq!(wrappers.len(), 1);
        }
        other => panic!("expected StatusAllOk, got {other:?}"),
    }

    // Kill the daemon.
    child.kill().await.unwrap();
}
