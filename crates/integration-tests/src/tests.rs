//! End-to-end integration tests.

use std::collections::HashMap;

use underwrap_protocol::message::*;

use crate::harness::*;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn one_wrapper_config() -> (HashMap<String, underwrap_protocol::config::WrapperDef>,
                             HashMap<String, underwrap_protocol::config::SecretDef>) {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "gh-authorized".to_string(),
        wrapper("/usr/bin/gh", "github/token", "GH_TOKEN"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("github/token".to_string(), secret(false));
    (wrappers, secrets)
}

fn multi_wrapper_config() -> (HashMap<String, underwrap_protocol::config::WrapperDef>,
                               HashMap<String, underwrap_protocol::config::SecretDef>) {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "gh-authorized".to_string(),
        wrapper("/usr/bin/gh", "github/token", "GH_TOKEN"),
    );
    wrappers.insert(
        "agent-slack".to_string(),
        wrapper("/usr/bin/slack-cli", "slack/xoxp", "SLACK_TOKEN"),
    );
    wrappers.insert(
        "gws-castlabs".to_string(),
        wrapper("/usr/bin/gws", "google/castlabs", "GOOGLE_WORKSPACE_CLI_TOKEN"),
    );

    let mut secrets = HashMap::new();
    secrets.insert("github/token".to_string(), secret(false));
    secrets.insert("slack/xoxp".to_string(), secret(false));
    secrets.insert("google/castlabs".to_string(), secret(true));
    (wrappers, secrets)
}

// ===========================================================================
// Store and read flow
// ===========================================================================

#[tokio::test]
async fn store_then_read_secret() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Store the secret.
    let resp = daemon.store_secret("github/token", "ghp_test123").await.unwrap();
    assert!(matches!(resp, ControlResponse::Ok { .. }));

    // Read it back through the read socket.
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        ReadResponse::Ok { material } => assert_eq!(material, "ghp_test123"),
        other => panic!("expected Ok, got {other:?}"),
    }

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
        ReadResponse::Error { code, message } => {
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
async fn read_unknown_wrapper() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let resp = daemon.read_request("nonexistent-wrapper").await.unwrap();
    match resp {
        ReadResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::NotFound));
            assert!(message.contains("nonexistent-wrapper"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

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

// ===========================================================================
// Multiple wrappers, isolation
// ===========================================================================

#[tokio::test]
async fn multiple_wrappers_isolate_secrets() {
    let (wrappers, secrets) = multi_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Store different secrets.
    daemon.store_secret("github/token", "ghp_AAA").await.unwrap();
    daemon.store_secret("slack/xoxp", "xoxp-BBB").await.unwrap();
    daemon.store_secret("google/castlabs", "goog-CCC").await.unwrap();

    // Each wrapper reads only its own secret.
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        ReadResponse::Ok { material } => assert_eq!(material, "ghp_AAA"),
        other => panic!("expected Ok, got {other:?}"),
    }

    let resp = daemon.read_request("agent-slack").await.unwrap();
    match resp {
        ReadResponse::Ok { material } => assert_eq!(material, "xoxp-BBB"),
        other => panic!("expected Ok, got {other:?}"),
    }

    let resp = daemon.read_request("gws-castlabs").await.unwrap();
    match resp {
        ReadResponse::Ok { material } => assert_eq!(material, "goog-CCC"),
        other => panic!("expected Ok, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Overwrite mutable secret
// ===========================================================================

#[tokio::test]
async fn overwrite_mutable_secret() {
    let (wrappers, secrets) = multi_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Store initial value.
    daemon.store_secret("google/castlabs", "refresh_v1").await.unwrap();
    let resp = daemon.read_request("gws-castlabs").await.unwrap();
    match resp {
        ReadResponse::Ok { material } => assert_eq!(material, "refresh_v1"),
        other => panic!("expected Ok, got {other:?}"),
    }

    // Overwrite.
    daemon.store_secret("google/castlabs", "refresh_v2").await.unwrap();
    let resp = daemon.read_request("gws-castlabs").await.unwrap();
    match resp {
        ReadResponse::Ok { material } => assert_eq!(material, "refresh_v2"),
        other => panic!("expected Ok, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Status — general
// ===========================================================================

#[tokio::test]
async fn status_general() {
    let (wrappers, secrets) = multi_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let resp = daemon.status(None).await.unwrap();
    match resp {
        ControlResponse::Ok { info: Some(info) } => {
            let wrapper_list = info["wrappers"].as_array().unwrap();
            assert_eq!(wrapper_list.len(), 3);
            let secret_list = info["secrets"].as_array().unwrap();
            assert_eq!(secret_list.len(), 3);
        }
        other => panic!("expected Ok with info, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Status — specific wrapper
// ===========================================================================

#[tokio::test]
async fn status_specific_wrapper() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Before storing: secret_stored = false.
    let resp = daemon.status(Some("gh-authorized")).await.unwrap();
    match resp {
        ControlResponse::Ok { info: Some(info) } => {
            assert_eq!(info["wrapper"], "gh-authorized");
            assert_eq!(info["secret_stored"], false);
        }
        other => panic!("expected Ok, got {other:?}"),
    }

    // After storing: secret_stored = true.
    daemon.store_secret("github/token", "tok").await.unwrap();
    let resp = daemon.status(Some("gh-authorized")).await.unwrap();
    match resp {
        ControlResponse::Ok { info: Some(info) } => {
            assert_eq!(info["secret_stored"], true);
        }
        other => panic!("expected Ok, got {other:?}"),
    }

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
    match resp {
        ControlResponse::Ok { info: Some(info) } => {
            assert_eq!(info["secret"], "github/token");
            assert_eq!(info["stored"], false);
        }
        other => panic!("expected Ok, got {other:?}"),
    }

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
        ControlResponse::Ok { info: Some(info) } => {
            assert!(info["error"].as_str().unwrap().contains("nope"));
        }
        other => panic!("expected Ok, got {other:?}"),
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

    daemon.store_secret("github/token", "ghp_concurrent").await.unwrap();

    // Spawn 20 concurrent read requests.
    let mut handles = Vec::new();
    for _ in 0..20 {
        let sock = daemon.read_socket.clone();
        handles.push(tokio::spawn(async move {
            let mut stream = tokio::net::UnixStream::connect(&sock).await.unwrap();
            let req = ReadRequest {
                wrapper: "gh-authorized".to_string(),
                scope: None,
            };
            underwrap_protocol::ipc::write_frame_async(&mut stream, &req)
                .await
                .unwrap();
            let resp: ReadResponse =
                underwrap_protocol::ipc::read_frame_async(&mut stream).await.unwrap();
            resp
        }));
    }

    for h in handles {
        let resp = h.await.unwrap();
        match resp {
            ReadResponse::Ok { material } => assert_eq!(material, "ghp_concurrent"),
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Secrets with slashes (hierarchical IDs)
// ===========================================================================

#[tokio::test]
async fn hierarchical_secret_ids() {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "deep-wrapper".to_string(),
        wrapper("/bin/echo", "org/team/service/token", "TOKEN"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("org/team/service/token".to_string(), secret(false));

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    daemon.store_secret("org/team/service/token", "deep_val").await.unwrap();

    let resp = daemon.read_request("deep-wrapper").await.unwrap();
    match resp {
        ReadResponse::Ok { material } => assert_eq!(material, "deep_val"),
        other => panic!("expected Ok, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Large secret material
// ===========================================================================

#[tokio::test]
async fn large_secret_material() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // ~32KB of data (simulating an OAuth JSON blob).
    let big = "x".repeat(32_000);
    daemon.store_secret("github/token", &big).await.unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        ReadResponse::Ok { material } => assert_eq!(material.len(), 32_000),
        other => panic!("expected Ok, got {other:?}"),
    }

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
    match resp {
        ReadResponse::Ok { material } => assert_eq!(material, special),
        other => panic!("expected Ok, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Binary-level wrapper test (underwrap-exec via process)
// ===========================================================================

#[tokio::test]
async fn exec_wrapper_end_to_end() {
    // We configure the daemon with "test-wrapper" as a wrapper name, using
    // `env` as the target binary — it prints all env vars to stdout.
    let env_bin = which_env();

    let mut wrappers = HashMap::new();
    wrappers.insert(
        "test-wrapper".to_string(),
        wrapper(&env_bin, "test/secret", "MY_SECRET"),
    );
    let mut secrets = HashMap::new();
    secrets.insert("test/secret".to_string(), secret(false));

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    daemon.store_secret("test/secret", "s3cret_value").await.unwrap();

    // Write the sidecar wrapper config for underwrap-exec.
    daemon.write_wrapper_config("test-wrapper", &env_bin, "MY_SECRET").unwrap();

    // Build the underwrap-exec binary path (from cargo target dir).
    let exec_bin = cargo_bin("underwrap-exec");

    // Invoke underwrap-exec with argv[0] set to "test-wrapper".
    let output = tokio::process::Command::new(&exec_bin)
        .arg0("test-wrapper")
        .env("UNDERWRAP_CONFIG_DIR", daemon.wrappers_dir.to_str().unwrap())
        .output()
        .await
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // `env` prints all env vars; MY_SECRET should be in there.
    assert!(
        stdout.contains("MY_SECRET=s3cret_value"),
        "expected MY_SECRET in env output.\nstdout: {stdout}\nstderr: {stderr}\nexit: {}",
        output.status,
    );

    daemon.stop().await.unwrap();
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

/// Find the path to a cargo-built binary.
fn cargo_bin(name: &str) -> String {
    // Cargo sets OUT_DIR for build scripts but not for test binaries.
    // The target directory is typically at the workspace root.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = std::path::Path::new(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    // Try debug profile first.
    let debug_path = workspace_root.join("target/debug").join(name);
    if debug_path.exists() {
        return debug_path.to_str().unwrap().to_string();
    }

    // Fallback: ask cargo.
    panic!(
        "binary '{name}' not found at {}. Run `cargo build` first.",
        debug_path.display()
    );
}

/// Find the `env` binary.
fn which_env() -> String {
    for p in &["/usr/bin/env", "/bin/env"] {
        if std::path::Path::new(p).exists() {
            return p.to_string();
        }
    }
    "env".to_string()
}
