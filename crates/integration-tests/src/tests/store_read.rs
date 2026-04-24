use super::*;

#[tokio::test]
async fn store_then_read_secret() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Store the secret.
    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();

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
        .store_secret("test", "gh-authorized", "github/token", "real_token")
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "real_token");

    daemon.stop().await.unwrap();
}

// NOTE: Multi-wrapper isolation (each wrapper can only read its own secret)
// requires real per-wrapper Unix groups and setgid wrappers. This is tested
// in the NixOS VM test. Unit tests run as a single user with one GID, so
// all wrappers resolve to the same identity.

// ===========================================================================
// Admin secret writes: gated by policy `store.write` operation
// ===========================================================================

// With no policy document at all (fresh store), the gate auto-allows so
// first-time provisioning works without an explicit grant. Repeated writes
// are still allowed under that bootstrap mode.
#[tokio::test]
async fn admin_write_allowed_without_policy_document() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "initial")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "overwrite_attempt")
        .await
        .unwrap();

    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "overwrite_attempt");

    daemon.stop().await.unwrap();
}

// Once a policy document exists but has no `store.write` grant for the
// (wrapper, credential) tuple, the admin endpoint refuses overwrites with
// 403. The original value is preserved. A grant on a different wrapper or a
// different operation does not satisfy the gate either.
#[tokio::test]
async fn admin_write_denied_without_store_write_grant() {
    use underwrap_protocol::policy::{
        PolicyDocument, PolicyEffect, PolicyRule, WrapperApprovalMode, WrapperPolicyState,
    };

    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Bootstrap value (no policy doc yet, so this is allowed).
    daemon
        .store_secret("test", "gh-authorized", "github/token", "initial")
        .await
        .unwrap();

    // Install a policy doc that has rules for reads but no store.write grant.
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                wrappers: vec![WrapperPolicyState {
                    wrapper: "gh-authorized".to_string(),
                    mode: WrapperApprovalMode::Unlocked { until_epoch: None },
                    reason: None,
                }],
                rules: vec![PolicyRule {
                    id: "allow-read".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "secret:github/token".to_string(),
                    operation: "secret.read".to_string(),
                    effect: PolicyEffect::Allow,
                }],
                grants: Vec::new(),
            },
        )
        .await
        .unwrap();

    // Overwrite must be refused now.
    let err = daemon
        .store_secret("test", "gh-authorized", "github/token", "blocked")
        .await
        .expect_err("admin write should be denied without a store.write grant");
    let msg = err.to_string();
    assert!(msg.contains("403"), "expected 403, got: {msg}");

    // Original value still served.
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "initial");

    daemon.stop().await.unwrap();
}

// With an active grant for (wrapper, cred:<id>, store.write) the overwrite
// goes through. Removing the grant blocks subsequent writes again. This is
// the operator-toggleable mutation state that replaces the old `mutable`
// schema attribute.
#[tokio::test]
async fn admin_write_allowed_with_store_write_grant() {
    use underwrap_protocol::policy::{
        OP_STORE_WRITE, PolicyDocument, PolicyEffect, PolicyGrant, PolicyRule,
        WrapperApprovalMode, WrapperPolicyState, credential_resource,
    };

    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "v1")
        .await
        .unwrap();

    let resource = credential_resource("github/token");
    let policy_with_grant = PolicyDocument {
        wrappers: vec![WrapperPolicyState {
            wrapper: "gh-authorized".to_string(),
            mode: WrapperApprovalMode::Unlocked { until_epoch: None },
            reason: None,
        }],
        rules: vec![PolicyRule {
            id: "allow-read".to_string(),
            wrapper: "gh-authorized".to_string(),
            resource: "secret:github/token".to_string(),
            operation: "secret.read".to_string(),
            effect: PolicyEffect::Allow,
        }],
        grants: vec![PolicyGrant {
            id: "mutate-now".to_string(),
            wrapper: "gh-authorized".to_string(),
            resource: resource.clone(),
            operation: OP_STORE_WRITE.to_string(),
            effect: PolicyEffect::Allow,
            starts_at_epoch: None,
            expires_at_epoch: None,
            reason: Some("test mutation window".to_string()),
        }],
    };
    daemon.put_policy("test", &policy_with_grant).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "v2")
        .await
        .unwrap();
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "v2");

    // Drop the grant; further writes denied again.
    daemon
        .put_policy(
            "test",
            &PolicyDocument {
                wrappers: vec![WrapperPolicyState {
                    wrapper: "gh-authorized".to_string(),
                    mode: WrapperApprovalMode::Unlocked { until_epoch: None },
                    reason: None,
                }],
                rules: vec![PolicyRule {
                    id: "allow-read".to_string(),
                    wrapper: "gh-authorized".to_string(),
                    resource: "secret:github/token".to_string(),
                    operation: "secret.read".to_string(),
                    effect: PolicyEffect::Allow,
                }],
                grants: Vec::new(),
            },
        )
        .await
        .unwrap();

    let err = daemon
        .store_secret("test", "gh-authorized", "github/token", "v3")
        .await
        .expect_err("admin write should be denied after grant removed");
    assert!(err.to_string().contains("403"), "{err}");

    // Value from the grant window is preserved.
    let resp = daemon.read_request("gh-authorized").await.unwrap();
    assert_eq!(expect_single_env_string(resp), "v2");

    daemon.stop().await.unwrap();
}

// The admin endpoint must refuse to overwrite the reserved policy-document
// secret id even when an operator naively crafts a request for it.
#[tokio::test]
async fn admin_write_rejects_policy_document_secret_id() {
    use underwrap_protocol::policy::POLICY_DOCUMENT_SECRET_ID;

    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let err = daemon
        .store_secret("test", "gh-authorized", POLICY_DOCUMENT_SECRET_ID, "tampered")
        .await
        .expect_err("policy document secret id must be rejected");
    let msg = err.to_string();
    assert!(msg.contains("400"), "expected 400, got: {msg}");

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
        .store_secret("test", "gh-authorized", "github/token", "ghp_concurrent")
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
    secrets.insert("org/team/service/token".to_string(), secret());

    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    daemon
        .store_secret("test", "deep-wrapper", "org/team/service/token", "deep_val")
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
    daemon
        .store_secret("test", "gh-authorized", "github/token", &big)
        .await
        .unwrap();

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
    daemon
        .store_secret("test", "gh-authorized", "github/token", special)
        .await
        .unwrap();

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
    secrets.insert("rapid/tok".to_string(), secret());
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    for i in 0..50 {
        let val = format!("tok_{i}");
        daemon
            .store_secret("test", "rapid-wrapper", "rapid/tok", &val)
            .await
            .unwrap();

        let resp = daemon.read_request("rapid-wrapper").await.unwrap();
        assert_eq!(expect_single_env_string(resp), val, "iteration {i}");
    }

    daemon.stop().await.unwrap();
}
