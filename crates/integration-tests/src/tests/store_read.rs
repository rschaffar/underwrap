use super::*;
use base64::Engine;
use std::time::Duration;
use underwrap_protocol::target_env::FORCED_TARGET_ENV_NAMES;

const RESERVED_ENV_REASON: &str = "environment variable name is reserved by underwrap hardening";
const RUNTIME_VALUE_MARKER: &str = "synthetic-runtime-value-marker";

fn assert_env_read_rejected_without_channel(socket_path: &Path, name: &str) {
    let mut control = StdUnixStream::connect(socket_path).unwrap();
    control
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write_frame_sync(
        &mut control,
        &WrapperRequest::Read {
            wrapper_name: "env-wrapper".to_string(),
            variant: underwrap_protocol::variant::WrapperVariant::Real,
        },
    )
    .unwrap();

    let response: WrapperResponse = read_frame_sync(&mut control).unwrap();
    match response {
        WrapperResponse::Error {
            code: ErrorCode::Internal,
            message,
        } => {
            assert!(message.contains(name), "{name}: {message}");
            assert!(message.contains(RESERVED_ENV_REASON), "{name}: {message}");
            assert!(!message.contains(RUNTIME_VALUE_MARKER), "{name}: {message}");
        }
        other => panic!("expected Internal error for {name}, got {other:?}"),
    }

    let error = scm_rights::recv_fd(control.as_fd()).unwrap_err();
    assert_eq!(error.to_string(), "recvmsg: connection closed", "{name}");
}

#[tokio::test]
async fn store_then_read_secret() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_test123")
        .await
        .unwrap();

    let (resp, blob, _) = checkout_resource(daemon.instance_socket.clone(), "gh-authorized").await;
    assert!(matches!(
        resp,
        WrapperResponse::ReadOk {
            env_projection: true,
            ..
        }
    ));
    assert_eq!(expect_single_env_string(blob), "ghp_test123");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn read_before_store_returns_empty_env() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    // Daemon registration seeds a `Locked` permission for every
    // wrapper-resource pair; unlock it so the test can exercise the
    // read path on a resource that has never been written.
    daemon
        .upsert_permission(
            "test",
            &daemon.current_permission("gh-authorized", "github/token"),
        )
        .await
        .unwrap();

    let (resp, blob, _) = checkout_resource(daemon.instance_socket.clone(), "gh-authorized").await;
    assert!(matches!(
        resp,
        WrapperResponse::ReadOk {
            env_projection: true,
            ..
        }
    ));
    assert!(blob.env.is_empty());

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn read_validates_client_supplied_wrapper_name() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "real_token")
        .await
        .unwrap();

    // A known wrapper name resolves and reads.
    let (_, blob, _) = checkout_resource(daemon.instance_socket.clone(), "gh-authorized").await;
    assert_eq!(expect_single_env_string(blob), "real_token");

    // An unknown client-supplied wrapper name is rejected by the daemon
    // (the name is now a client-supplied selector validated against the
    // instance config, not bound to a per-wrapper socket). Connect to
    // the shared instance socket and send a bogus name.
    let mut stream = tokio::net::UnixStream::connect(&daemon.instance_socket)
        .await
        .unwrap();
    underwrap_protocol::ipc::write_frame_async(
        &mut stream,
        &WrapperRequest::Read {
            wrapper_name: "ghost-wrapper".to_string(),
            variant: underwrap_protocol::variant::WrapperVariant::Real,
        },
    )
    .await
    .unwrap();
    let resp: WrapperResponse = underwrap_protocol::ipc::read_frame_async(&mut stream)
        .await
        .unwrap();
    assert!(
        matches!(resp, WrapperResponse::Error { ref message, .. } if message.contains("unknown wrapper")),
        "expected unknown-wrapper error, got {resp:?}",
    );

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn admin_writes_are_authenticated_operator_actions() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "initial")
        .await
        .unwrap();
    daemon
        .store_secret("test", "gh-authorized", "github/token", "overwrite_attempt")
        .await
        .unwrap();

    let (_, blob, _) = checkout_resource(daemon.instance_socket.clone(), "gh-authorized").await;
    assert_eq!(expect_single_env_string(blob), "overwrite_attempt");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn concurrent_reads() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    daemon
        .store_secret("test", "gh-authorized", "github/token", "ghp_concurrent")
        .await
        .unwrap();

    let mut handles = Vec::new();
    for _ in 0..20 {
        let sock = daemon.instance_socket.clone();
        handles.push(tokio::spawn(async move {
            checkout_resource(sock, "gh-authorized").await
        }));
    }

    for h in handles {
        let (resp, blob, _) = h.await.unwrap();
        assert!(matches!(
            resp,
            WrapperResponse::ReadOk {
                env_projection: true,
                ..
            }
        ));
        assert_eq!(expect_single_env_string(blob), "ghp_concurrent");
    }

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn hierarchical_secret_ids() {
    let echo = require_echo!();
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "deep-wrapper".to_string(),
        wrapper(&echo, "org/team/service/token", "TOKEN"),
    );

    let daemon = TestDaemon::start(wrappers).await.unwrap();

    daemon
        .store_secret("test", "deep-wrapper", "org/team/service/token", "deep_val")
        .await
        .unwrap();

    let (_, blob, _) = checkout_resource(daemon.instance_socket.clone(), "deep-wrapper").await;
    assert_eq!(expect_single_env_string(blob), "deep_val");

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn large_secret_material() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    let big = "x".repeat(32_000);
    daemon
        .store_secret("test", "gh-authorized", "github/token", &big)
        .await
        .unwrap();

    let (_, blob, _) = checkout_resource(daemon.instance_socket.clone(), "gh-authorized").await;
    assert_eq!(expect_single_env_bytes(blob).len(), 32_000);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn secret_with_special_chars() {
    let wrappers = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    let special = "tok=\"abc 123\"\nnewline\ttab\0null";
    daemon
        .store_secret("test", "gh-authorized", "github/token", special)
        .await
        .unwrap();

    let (_, blob, _) = checkout_resource(daemon.instance_socket.clone(), "gh-authorized").await;
    assert_eq!(expect_single_env_bytes(blob), special.as_bytes());

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn forced_target_env_names_fail_closed_and_recover() {
    let resource = "forced-env-resource";
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "env-wrapper".to_string(),
        wrapper("/usr/bin/true", resource, "IGNORED"),
    );
    wrappers.insert(
        "files-only-wrapper".to_string(),
        files_wrapper("/usr/bin/true", resource, "projected"),
    );
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    daemon
        .store_secret("test", "env-wrapper", resource, "ordinary-resource-value")
        .await
        .unwrap();
    daemon
        .upsert_permission(
            "test",
            &daemon.current_permission("files-only-wrapper", resource),
        )
        .await
        .unwrap();
    let ordinary = daemon.get_resource_blob("test", resource).unwrap();
    let ordinary_bytes = ordinary.to_canonical_bytes().unwrap();

    for name in FORCED_TARGET_ENV_NAMES {
        let response = daemon
            .admin_request(
                reqwest::Method::PUT,
                &format!("/v1/admin/stores/test/secrets/{resource}/env/{name}"),
            )
            .json(&underwrap_protocol::service::AdminSecretWriteRequest {
                material_b64: base64::engine::general_purpose::STANDARD
                    .encode(b"synthetic-admin-rejected-material"),
            })
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            reqwest::StatusCode::BAD_REQUEST,
            "{name}"
        );
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "code": "invalid_request",
                "message": RESERVED_ENV_REASON,
            }),
            "{name}",
        );
        assert!(body.get("material_b64").is_none(), "{name}: {body}");
        assert!(!body.to_string().contains("synthetic-admin"), "{name}");
        assert_eq!(
            daemon
                .get_resource_blob("test", resource)
                .unwrap()
                .to_canonical_bytes()
                .unwrap(),
            ordinary_bytes,
            "{name}",
        );
    }

    for name in FORCED_TARGET_ENV_NAMES {
        let mut rejected_value = RUNTIME_VALUE_MARKER.as_bytes().to_vec();
        rejected_value.push(0xff);
        let mut invalid = ordinary.clone();
        invalid.env.insert(
            (*name).to_string(),
            underwrap_protocol::message::SecretBytes::from(rejected_value),
        );
        daemon
            .put_resource_blob("test", resource, &invalid)
            .unwrap();
        let invalid_bytes = invalid.to_canonical_bytes().unwrap();

        let response = daemon
            .admin_request(
                reqwest::Method::DELETE,
                &format!("/v1/admin/stores/test/secrets/{resource}/env/{name}"),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            reqwest::StatusCode::BAD_REQUEST,
            "{name}"
        );
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "code": "invalid_request",
                "message": RESERVED_ENV_REASON,
            }),
            "{name}",
        );
        assert!(!body.to_string().contains(RUNTIME_VALUE_MARKER), "{name}");
        assert_eq!(
            daemon
                .get_resource_blob("test", resource)
                .unwrap()
                .to_canonical_bytes()
                .unwrap(),
            invalid_bytes,
            "{name}",
        );

        let blocked = daemon.delete_resource("test", resource).await.unwrap();
        assert!(!blocked.ok, "{name}: invalid content must block deletion");
        let blockers = blocked.blocked_by.expect("content blockers present");
        assert!(blockers.content_entry_count >= 1, "{name}");
        assert!(
            blockers.sample_content.iter().any(|entry| matches!(
                entry,
                underwrap_protocol::resource_blob::ResourceContentReference::Env {
                    name: blocked_name,
                } if blocked_name == name
            )),
            "{name}: malformed env entry absent from {blockers:?}",
        );
        assert_eq!(
            daemon
                .get_resource_blob("test", resource)
                .unwrap()
                .to_canonical_bytes()
                .unwrap(),
            invalid_bytes,
            "{name}",
        );

        let socket = daemon.instance_socket.clone();
        let rejected_name = (*name).to_string();
        tokio::task::spawn_blocking(move || {
            assert_env_read_rejected_without_channel(&socket, &rejected_name)
        })
        .await
        .unwrap();

        let (response, files_blob, _) =
            checkout_resource(daemon.instance_socket.clone(), "files-only-wrapper").await;
        assert!(matches!(
            response,
            WrapperResponse::ReadOk {
                env_projection: false,
                ..
            }
        ));
        assert_eq!(files_blob, invalid, "{name}");

        let near_miss = format!("{name}_APP");
        let mut recovered = ordinary.clone();
        recovered.env.insert(
            near_miss,
            underwrap_protocol::message::SecretBytes::from("accepted-near-miss"),
        );
        // Test-only trusted restoration simulation: production admin operations
        // intentionally cannot repair this outside-contract stored state.
        daemon
            .put_resource_blob("test", resource, &recovered)
            .unwrap();
        let (response, env_blob, _) =
            checkout_resource(daemon.instance_socket.clone(), "env-wrapper").await;
        assert!(matches!(
            response,
            WrapperResponse::ReadOk {
                env_projection: true,
                ..
            }
        ));
        assert_eq!(env_blob, recovered, "{name}");
    }

    daemon
        .put_resource_blob("test", resource, &ordinary)
        .unwrap();
    let (response, recovered, _) =
        checkout_resource(daemon.instance_socket.clone(), "env-wrapper").await;
    assert!(matches!(
        response,
        WrapperResponse::ReadOk {
            env_projection: true,
            ..
        }
    ));
    assert_eq!(recovered.to_canonical_bytes().unwrap(), ordinary_bytes);

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn rapid_store_read_cycles() {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "rapid-wrapper".to_string(),
        wrapper("/usr/bin/true", "rapid/tok", "TOK"),
    );
    let daemon = TestDaemon::start(wrappers).await.unwrap();

    for i in 0..50 {
        let val = format!("tok_{i}");
        daemon
            .store_secret("test", "rapid-wrapper", "rapid/tok", &val)
            .await
            .unwrap();

        let (_, blob, _) = checkout_resource(daemon.instance_socket.clone(), "rapid-wrapper").await;
        assert_eq!(expect_single_env_string(blob), val, "iteration {i}");
    }

    daemon.stop().await.unwrap();
}
