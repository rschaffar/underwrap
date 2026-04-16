use super::*;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};

#[tokio::test]
async fn wrapper_ssh_signature_approval_same_connection() {
    use ssh_key::PrivateKey;
    use ssh_key::private::{Ed25519Keypair, KeypairData};

    let keypair = Ed25519Keypair::random(&mut rand::thread_rng());
    let private_key = PrivateKey::new(KeypairData::Ed25519(keypair.clone()), "test-key").unwrap();
    let public_key = private_key.public_key();
    let public_key_openssh = public_key.to_openssh().unwrap();
    let public_key_blob = public_key.to_bytes().unwrap();

    let mut wrappers = HashMap::new();
    let mut w = locked_wrapper("/usr/bin/true", "key-locked/tok", "TOK");
    w.unlock_key = Some(public_key_openssh);
    wrappers.insert("key-locked-w".to_string(), w);

    let mut secrets = HashMap::new();
    secrets.insert("key-locked/tok".to_string(), secret(false));

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();
    daemon
        .store_secret("test", "key-locked/tok", "key_secret")
        .await
        .unwrap();

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

    let signing_key = ed25519_dalek::SigningKey::from_bytes(&keypair.private.to_bytes());
    use ed25519_dalek::Signer;
    let raw_sig = signing_key.sign(&nonce);

    let algo = b"ssh-ed25519";
    let sig_bytes = raw_sig.to_bytes();
    let mut wire_sig = Vec::new();
    wire_sig.extend_from_slice(&(algo.len() as u32).to_be_bytes());
    wire_sig.extend_from_slice(algo);
    wire_sig.extend_from_slice(&(sig_bytes.len() as u32).to_be_bytes());
    wire_sig.extend_from_slice(&sig_bytes);

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

    let resp = daemon.read_request("key-locked-w").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "key_secret");

    daemon.stop().await.unwrap();
}
