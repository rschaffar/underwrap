use super::*;

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
