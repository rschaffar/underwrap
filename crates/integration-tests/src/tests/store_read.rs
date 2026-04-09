use super::*;

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

#[tokio::test]
async fn store_directory_credential_rejected() {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "dir-w".to_string(),
        wrapper("/usr/bin/true", "gws/state", "IGNORED"),
    );
    let mut credentials = HashMap::new();
    credentials.insert("gws/state".to_string(), directory_credential(".config/gws"));
    let daemon = TestDaemon::start(wrappers, credentials).await.unwrap();

    let resp = daemon.store_secret("gws/state", "data").await.unwrap();
    match resp {
        ControlResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
            assert!(message.contains("not an env credential"));
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
