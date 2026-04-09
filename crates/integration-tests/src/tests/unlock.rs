use super::*;

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
