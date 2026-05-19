use super::*;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode};

/// Sign `nonce` with `signing_key` and wrap the raw Ed25519 signature as the
/// SSH agent wire format that `ssh_key::Signature::try_from` expects.
fn sign_nonce(signing_key: &ed25519_dalek::SigningKey, nonce: &[u8]) -> Vec<u8> {
    use ed25519_dalek::Signer;
    let raw = signing_key.sign(nonce).to_bytes();
    let algo = b"ssh-ed25519";
    let mut wire = Vec::with_capacity(4 + algo.len() + 4 + raw.len());
    wire.extend_from_slice(&(algo.len() as u32).to_be_bytes());
    wire.extend_from_slice(algo);
    wire.extend_from_slice(&(raw.len() as u32).to_be_bytes());
    wire.extend_from_slice(&raw);
    wire
}

#[tokio::test]
async fn per_call_approval_succeeds_with_valid_ed25519_signature() {
    use ssh_key::PrivateKey;
    use ssh_key::private::{Ed25519Keypair, KeypairData};

    let keypair = Ed25519Keypair::random(&mut rand::thread_rng());
    let private_key = PrivateKey::new(KeypairData::Ed25519(keypair.clone()), "test-key").unwrap();
    let public_key = private_key.public_key();
    let public_key_openssh = public_key.to_openssh().unwrap();
    let public_key_blob = public_key.to_bytes().unwrap();
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&keypair.private.to_bytes());

    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();
    let mut grant = daemon.current_grant("gh-authorized", "github/token");
    grant.mode = WrapperApprovalMode::PerCallApproval {
        approval_key: public_key_openssh,
    };
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                grants: vec![grant],
            },
        )
        .await
        .unwrap();

    let mut stream = daemon.connect_wrapper("gh-authorized").await.unwrap();
    write_frame_async(&mut stream, &WrapperRequest::Read { scope: None })
        .await
        .unwrap();
    let resp: WrapperResponse = read_frame_async(&mut stream).await.unwrap();
    let (challenge_id, nonce) = match resp {
        WrapperResponse::ApprovalRequired {
            challenge_id,
            nonce,
            ..
        } => (challenge_id, nonce),
        other => panic!("expected ApprovalRequired, got {other:?}"),
    };

    let signature = sign_nonce(&signing_key, &nonce);
    write_frame_async(
        &mut stream,
        &WrapperRequest::ApprovalProof {
            challenge_id,
            signature,
            public_key_blob,
        },
    )
    .await
    .unwrap();

    let resp: WrapperResponse = read_frame_async(&mut stream).await.unwrap();
    assert_eq!(expect_single_env_string(resp), "ghp_test123");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn per_call_approval_rejects_wrong_signature() {
    use ssh_key::PrivateKey;
    use ssh_key::private::{Ed25519Keypair, KeypairData};

    let authorized = Ed25519Keypair::random(&mut rand::thread_rng());
    let authorized_priv =
        PrivateKey::new(KeypairData::Ed25519(authorized.clone()), "authorized").unwrap();
    let authorized_openssh = authorized_priv.public_key().to_openssh().unwrap();

    let other = Ed25519Keypair::random(&mut rand::thread_rng());
    let other_priv = PrivateKey::new(KeypairData::Ed25519(other.clone()), "other").unwrap();
    let other_pubkey_blob = other_priv.public_key().to_bytes().unwrap();
    let other_signing = ed25519_dalek::SigningKey::from_bytes(&other.private.to_bytes());

    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();
    let mut grant = daemon.current_grant("gh-authorized", "github/token");
    grant.mode = WrapperApprovalMode::PerCallApproval {
        approval_key: authorized_openssh,
    };
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                grants: vec![grant],
            },
        )
        .await
        .unwrap();

    let mut stream = daemon.connect_wrapper("gh-authorized").await.unwrap();
    write_frame_async(&mut stream, &WrapperRequest::Read { scope: None })
        .await
        .unwrap();
    let resp: WrapperResponse = read_frame_async(&mut stream).await.unwrap();
    let (challenge_id, nonce) = match resp {
        WrapperResponse::ApprovalRequired {
            challenge_id,
            nonce,
            ..
        } => (challenge_id, nonce),
        other => panic!("expected ApprovalRequired, got {other:?}"),
    };

    // Sign with the *wrong* key. The daemon must reject.
    let signature = sign_nonce(&other_signing, &nonce);
    write_frame_async(
        &mut stream,
        &WrapperRequest::ApprovalProof {
            challenge_id,
            signature,
            public_key_blob: other_pubkey_blob,
        },
    )
    .await
    .unwrap();

    let resp: WrapperResponse = read_frame_async(&mut stream).await.unwrap();
    match resp {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
            assert!(message.contains("approval"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn per_call_approval_denies_without_matching_grant() {
    use ssh_key::PrivateKey;
    use ssh_key::private::{Ed25519Keypair, KeypairData};

    let keypair = Ed25519Keypair::random(&mut rand::thread_rng());
    let private_key = PrivateKey::new(KeypairData::Ed25519(keypair.clone()), "k").unwrap();
    let pubkey_openssh = private_key.public_key().to_openssh().unwrap();

    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();
    // Policy has per-call approval mode for a DIFFERENT client
    // instance; this daemon's instance has no grant at all and must
    // be rejected before the approval round even fires.
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                grants: vec![underwrap_protocol::policy::AgentGrant {
                    client_instance: "some-other-instance".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "github/token".to_string(),
                    mode: WrapperApprovalMode::PerCallApproval {
                        approval_key: pubkey_openssh,
                    },
                    writable: false,
                    reason: None,
                }],
            },
        )
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    match resp {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
            assert!(message.contains("no grant"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}
