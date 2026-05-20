use super::*;
use crate::harness::{pick_unused_port, wait_for_service};
use base64::Engine;

#[tokio::test]
async fn parse_example_config() {
    // Inline a representative config matching the structure of examples/config.yaml.
    // This avoids depending on the file being present in nix sandbox builds.
    let text = r#"
backend:
  url: https://backend.example.internal:8443
  ca_file: /etc/underwrap/ca.pem
  client_cert_file: /etc/underwrap/client.pem
  client_key_file: /etc/underwrap/client.key
wrappers:
  gh-authorized:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-authorized.sock
    group: underwrap-gh-authorized
    resource: github/token
    env: true
  agent-slack:
    target: /usr/bin/slack-cli
    socket: /run/underwrap/agent-slack.sock
    group: underwrap-agent-slack
    resource: slack/xoxp
    env: true
  gh-libsecret:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-libsecret.sock
    group: underwrap-gh-libsecret
    resource: github/gh
    libsecret: true
"#;
    let config: underwrap_protocol::config::DaemonConfig = serde_yaml::from_str(text).unwrap();

    assert_eq!(config.wrappers.len(), 3);
    assert!(config.wrappers.contains_key("gh-authorized"));
    assert!(config.wrappers.contains_key("agent-slack"));
    assert!(config.wrappers.contains_key("gh-libsecret"));

    let gh = &config.wrappers["gh-authorized"];
    assert_eq!(gh.resource, "github/token");
    assert!(gh.env);
    let lib = &config.wrappers["gh-libsecret"];
    assert!(lib.libsecret);
}

// ===========================================================================
// underwrapd binary: starts and can read via wrapper using backend admin-seeded state
// ===========================================================================

#[tokio::test]
async fn backend_admin_starts_from_empty_dynamic_state() {
    let backend_bin = cargo_bin("underwrap-backendd");
    let https_port = pick_unused_port().unwrap();
    let admin_url = format!("https://127.0.0.1:{https_port}");

    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let server_cert = base.join("server.pem");
    let server_key = base.join("server.key");
    let ca_cert = base.join("ca.pem");
    let session_key_file = base.join("admin-session-key");
    std::fs::write(&server_cert, crate::harness::TEST_SERVER_PEM).unwrap();
    std::fs::write(&server_key, crate::harness::TEST_SERVER_KEY).unwrap();
    std::fs::write(&ca_cert, crate::harness::TEST_CA_PEM).unwrap();
    std::fs::write(
        &session_key_file,
        [5u8; underwrap_backendd::auth::SESSION_KEY_LEN],
    )
    .unwrap();
    let backend_config = serde_yaml::to_string(&underwrap_backendd::server::BackendConfig {
        stores_file: Some(base.join("stores.json")),
        client_instances_file: Some(base.join("client-instances.json")),
        operators_file: Some(base.join("operators.json")),
        https: Some(underwrap_backendd::server::HttpsConfig {
            listen: format!("127.0.0.1:{https_port}"),
            tls: underwrap_backendd::server::HttpsTlsConfig {
                cert_file: server_cert,
                key_file: server_key,
            },
        }),
        admin: Some(underwrap_backendd::server::AdminConfig {
            allowed_origins: None,
            session_key_file,
            session_lifetime_secs: None,
            stores_data_root: Some(base.join("stores-data-root")),
            keys_dir: Some(base.join("keys")),
        }),
    })
    .unwrap();
    let backend_config_path = base.join("backend-empty.yaml");
    std::fs::write(&backend_config_path, &backend_config).unwrap();

    let mut backend_child = tokio::process::Command::new(&backend_bin)
        .args(["--config", backend_config_path.to_str().unwrap()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .add_root_certificate(
            reqwest::Certificate::from_pem(&std::fs::read(&ca_cert).unwrap()).unwrap(),
        )
        .build()
        .unwrap();
    let mut bootstrap_required = None;
    for _ in 0..100 {
        if let Ok(resp) = client
            .get(format!("{admin_url}/v1/admin/auth/bootstrap/status"))
            .send()
            .await
            && let Ok(body) = resp.json::<serde_json::Value>().await
        {
            bootstrap_required = body.get("bootstrap_required").and_then(|v| v.as_bool());
            if bootstrap_required.is_some() {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    let _ = backend_child.start_kill();
    let backend_output = backend_child.wait_with_output().await.unwrap();
    assert_eq!(
        bootstrap_required,
        Some(true),
        "backend failed to boot from empty dynamic state\n--- backend stdout ---\n{}\n--- backend stderr ---\n{}",
        String::from_utf8_lossy(&backend_output.stdout),
        String::from_utf8_lossy(&backend_output.stderr),
    );
}

#[tokio::test]
async fn underwrapd_binary_start_and_read() {
    let daemon_bin = cargo_bin("underwrapd");
    let backend_bin = cargo_bin("underwrap-backendd");
    let https_port = pick_unused_port().unwrap();
    let service_url = format!("https://127.0.0.1:{https_port}");
    let admin_url = service_url.clone();

    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();

    let wrapper_sock = base.join("wrapper-w.sock");
    let server_cert = base.join("server.pem");
    let server_key = base.join("server.key");
    let ca_cert = base.join("ca.pem");
    let client_cert = base.join("client.pem");
    let client_key = base.join("client.key");
    let session_key_file = base.join("admin-session-key");
    std::fs::write(&server_cert, crate::harness::TEST_SERVER_PEM).unwrap();
    std::fs::write(&server_key, crate::harness::TEST_SERVER_KEY).unwrap();
    std::fs::write(&ca_cert, crate::harness::TEST_CA_PEM).unwrap();
    std::fs::write(&client_cert, crate::harness::TEST_CLIENT_LOCAL_PEM).unwrap();
    std::fs::write(&client_key, crate::harness::TEST_CLIENT_LOCAL_KEY).unwrap();
    std::fs::write(
        &session_key_file,
        [6u8; underwrap_backendd::auth::SESSION_KEY_LEN],
    )
    .unwrap();

    let stores_file = base.join("stores.json");
    let client_instances_file = base.join("client-instances.json");
    let test_store = underwrap_protocol::config::StoreRecord {
        id: "test".to_string(),
        name: "test".to_string(),
        description: None,
        created_at: None,
        storage: underwrap_protocol::config::StorageConfig::Plain {
            dir: base.join("secrets"),
        },
    };
    std::fs::write(
        &stores_file,
        serde_json::to_vec_pretty(&underwrap_protocol::config::StoreRegistryDocument {
            version: 1,
            stores: vec![test_store],
        })
        .unwrap(),
    )
    .unwrap();
    let client_cert_pem = std::fs::read_to_string(&client_cert).unwrap();
    let client_instance = underwrap_backendd::runtime_clients::ClientInstanceRecord {
        id: "inst_underwrap_local".to_string(),
        display_name: "underwrap-local".to_string(),
        current_store_id: "test".to_string(),
        status: underwrap_backendd::runtime_clients::ClientInstanceStatus::Active,
        last_seen_at: None,
        current_credential: underwrap_backendd::runtime_clients::current_credential_from_pem(
            client_cert_pem,
            None,
        )
        .unwrap(),
        auto_create_wrapper_assets: false,
    };
    std::fs::write(
        &client_instances_file,
        serde_json::to_vec_pretty(
            &underwrap_backendd::runtime_clients::ClientInstancesDocument {
                version: 1,
                instances: vec![client_instance],
            },
        )
        .unwrap(),
    )
    .unwrap();
    let backend_config = serde_yaml::to_string(&underwrap_backendd::server::BackendConfig {
        stores_file: Some(stores_file),
        client_instances_file: Some(client_instances_file),
        operators_file: Some(base.join("operators.json")),
        https: Some(underwrap_backendd::server::HttpsConfig {
            listen: format!("127.0.0.1:{https_port}"),
            tls: underwrap_backendd::server::HttpsTlsConfig {
                cert_file: server_cert.clone(),
                key_file: server_key.clone(),
            },
        }),
        admin: Some(underwrap_backendd::server::AdminConfig {
            allowed_origins: None,
            session_key_file: session_key_file.clone(),
            session_lifetime_secs: None,
            stores_data_root: None,
            keys_dir: None,
        }),
    })
    .unwrap();
    let backend_config_path = base.join("backend.yaml");
    std::fs::write(&backend_config_path, &backend_config).unwrap();

    let mut backend_child = tokio::process::Command::new(&backend_bin)
        .args(["--config", backend_config_path.to_str().unwrap()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    wait_for_admin(&admin_url, &ca_cert).await.unwrap();
    let (admin_session_cookie, admin_csrf_token) =
        bootstrap_admin(&admin_url, &ca_cert, "tester", "hunter2")
            .await
            .unwrap();
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .add_root_certificate(
            reqwest::Certificate::from_pem(&std::fs::read(&ca_cert).unwrap()).unwrap(),
        )
        .build()
        .unwrap();
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        wait_for_service(&service_url, &ca_cert, &client_cert, &client_key),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(e)) => panic!("service listener readiness failed: {e:#}"),
        Err(_) => {
            let _ = backend_child.start_kill();
            let backend_output = backend_child.wait_with_output().await.unwrap();
            panic!(
                "service listener readiness timed out\n--- backend stdout ---\n{}\n--- backend stderr ---\n{}",
                String::from_utf8_lossy(&backend_output.stdout),
                String::from_utf8_lossy(&backend_output.stderr),
            );
        }
    }

    let config = serde_yaml::to_string(&underwrap_protocol::config::DaemonConfig {
        backend: underwrap_protocol::config::BackendServiceConfig {
            url: service_url.clone(),
            ca_file: Some(ca_cert.clone()),
            client_cert_file: Some(client_cert.clone()),
            client_key_file: Some(client_key.clone()),
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

    for _ in 0..100 {
        if wrapper_sock.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(wrapper_sock.exists(), "wrapper socket never appeared");

    // Admin writes are unconditional now that per-resource write
    // flags only gate wrapper-side writes — drop the secret in
    // directly.
    let write_resp = client
        .post(format!("{admin_url}/v1/admin/secret/write"))
        .header(
            reqwest::header::COOKIE,
            csrf_cookie_header(&admin_session_cookie, &admin_csrf_token),
        )
        .header("X-CSRF-Token", &admin_csrf_token)
        .json(&underwrap_protocol::service::AdminSecretWriteRequest {
            store: "test".to_string(),
            wrapper: "w".to_string(),
            secret_id: "s/t/env/TEST_TOKEN".to_string(),
            material_b64: base64::engine::general_purpose::STANDARD.encode("value"),
        })
        .send()
        .await
        .unwrap();
    assert!(write_resp.status().is_success());

    // Grant the current Unix user (the cargo-test runner) an
    // unlocked grant on wrapper `w` for the host the daemon was
    // configured with (`binary-test`).
    let policy_resp = client
        .post(format!("{admin_url}/v1/admin/policy/put"))
        .header(
            reqwest::header::COOKIE,
            csrf_cookie_header(&admin_session_cookie, &admin_csrf_token),
        )
        .header("X-CSRF-Token", &admin_csrf_token)
        .json(&underwrap_protocol::service::AdminPolicyPutRequest {
            store: "test".to_string(),
            document: underwrap_protocol::policy::PolicyDocument {
                permissions: vec![underwrap_protocol::policy::WrapperPermission {
                    client_instance: "inst_underwrap_local".to_string(),
                    wrapper: "w".to_string(),
                    resource: "s/t".to_string(),
                    mode: underwrap_protocol::policy::WrapperApprovalMode::Unlocked,
                    writable: false,
                    reason: None,
                }],
            },
        })
        .send()
        .await
        .unwrap();
    assert!(policy_resp.status().is_success());

    let mut stream = tokio::net::UnixStream::connect(&wrapper_sock)
        .await
        .unwrap();
    let req = WrapperRequest::Read { scope: None };
    underwrap_protocol::ipc::write_frame_async(&mut stream, &req)
        .await
        .unwrap();
    let resp: WrapperResponse = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        underwrap_protocol::ipc::read_frame_async(&mut stream),
    )
    .await
    {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => panic!("wrapper read failed: {e:#}"),
        Err(_) => {
            let _ = child.start_kill();
            let _ = backend_child.start_kill();
            panic!("wrapper read timed out");
        }
    };

    assert_eq!(expect_single_env_string(resp), "value");

    child.start_kill().unwrap();
    backend_child.start_kill().unwrap();
}
