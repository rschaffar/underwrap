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
instance: local
socket: /run/underwrap/local.sock
group: underwrap-local
ns_binary: /run/wrappers/bin/underwrap-ns
debug_tool_target: /run/current-system/sw/bin/underwrap-debug-tool
xdg_runtime_dir_base: /run/user
wrappers:
  gh-authorized:
    target: /usr/bin/gh
    resource: github/token
    env: true
  agent-slack:
    target: /usr/bin/slack-cli
    resource: slack/xoxp
    env: true
  gh-libsecret:
    target: /usr/bin/gh
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
    std::fs::write(&server_cert, crate::harness::TEST_SERVER_PEM).unwrap();
    std::fs::write(&server_key, crate::harness::TEST_SERVER_KEY).unwrap();
    std::fs::write(&ca_cert, crate::harness::TEST_CA_PEM).unwrap();
    let backend_config = serde_yaml::to_string(&underwrap_backendd::BackendConfig {
        stores_dir: base.join("stores"),
        clients_dir: base.join("clients"),
        users_dir: base.join("users"),
        bootstrap_file: base.join("bootstrap.json"),
        https: Some(underwrap_backendd::HttpsConfig {
            listen: format!("127.0.0.1:{https_port}"),
            tls: underwrap_backendd::HttpsTlsConfig {
                cert_file: server_cert,
                key_file: server_key,
            },
        }),
        admin: Some(underwrap_backendd::AdminConfig {
            allowed_origins: Vec::new(),
            session_lifetime_secs: underwrap_backendd::DEFAULT_SESSION_LIFETIME_SECS,
            stores_data_root: Some(base.join("stores-data-root")),
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
    std::fs::write(&server_cert, crate::harness::TEST_SERVER_PEM).unwrap();
    std::fs::write(&server_key, crate::harness::TEST_SERVER_KEY).unwrap();
    std::fs::write(&ca_cert, crate::harness::TEST_CA_PEM).unwrap();
    std::fs::write(&client_cert, crate::harness::TEST_CLIENT_LOCAL_PEM).unwrap();
    std::fs::write(&client_key, crate::harness::TEST_CLIENT_LOCAL_KEY).unwrap();
    // The daemon refuses to start unless its mTLS credential is 0600 or
    // tighter and its parent directory is 0700 or tighter
    // (startup::check_credential_permissions), matching the NixOS module's
    // 0700 stateDir + keygen's 0600 output. Both `std::fs::write` and
    // `tempfile` honor the umask (0644 / 0755 in the Nix build sandbox),
    // so pin them explicitly.
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(base, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&client_cert, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(&client_key, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let stores_dir = base.join("stores");
    let clients_dir = base.join("clients");
    let test_store = underwrap_protocol::config::StoreRecord {
        id: "test".to_string(),
        name: "test".to_string(),
        description: None,
        created_at: None,
        storage: underwrap_protocol::config::StorageConfig::Plain {
            dir: base.join("secrets"),
        },
    };
    underwrap_backendd::store_registry::save_store_record(&stores_dir, &test_store).unwrap();
    let client_cert_pem = std::fs::read_to_string(&client_cert).unwrap();
    let client_instance = underwrap_backendd::runtime_clients::ClientInstanceRecord {
        id: "inst_underwrap_local".to_string(),
        display_name: "underwrap-local".to_string(),
        current_store_id: "test".to_string(),
        status: underwrap_backendd::runtime_clients::ClientInstanceStatus::Active,
        last_seen_at: None,
        current_credential: underwrap_backendd::runtime_clients::current_credential_from_pem(
            client_cert_pem,
        )
        .unwrap(),
        auto_create_wrapper_assets: false,
    };
    underwrap_backendd::runtime_clients::save_client_instance(&clients_dir, &client_instance)
        .unwrap();

    // Seed the resource + pair documents directly in the store. The
    // new resource-vocabulary backend resolves the wrapper's resource
    // by name and refuses the open when no `(wrapper, resource)` pair
    // exists. Auto-create is off in this test; the backend stores
    // these documents in the same plain directory.
    let secrets_dir = base.join("secrets");
    std::fs::create_dir_all(&secrets_dir).unwrap();
    // Seed the control-plane documents directly on disk. PlainBackend
    // maps a key 1:1 onto a relative file path, so the reserved
    // `control/<doc>` keys land at `<secrets_dir>/control/<doc>`.
    let write_document = |secret_id: &str, json: Vec<u8>| {
        let path = secrets_dir.join(secret_id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, json).unwrap();
    };
    write_document(
        underwrap_protocol::entities::RESOURCES_DOCUMENT_SECRET_ID,
        serde_json::to_vec_pretty(&underwrap_protocol::entities::ResourcesDocument {
            resources: vec![underwrap_protocol::entities::ResourceRecord {
                id: "s/t".into(),
                name: "s/t".into(),
                created_by: underwrap_protocol::entities::EntityCreator::Operator {
                    id: "test-op".into(),
                    username: "tester".into(),
                },
                created_at: 1,
            }],
        })
        .unwrap(),
    );
    write_document(
        underwrap_protocol::entities::PAIRS_DOCUMENT_SECRET_ID,
        serde_json::to_vec_pretty(&underwrap_protocol::entities::PairsDocument {
            pairs: vec![underwrap_protocol::entities::WrapperResourcePairRecord {
                wrapper: "w".into(),
                resource_id: "s/t".into(),
                created_by: underwrap_protocol::entities::EntityCreator::Operator {
                    id: "test-op".into(),
                    username: "tester".into(),
                },
                created_at: 1,
                last_used_at: None,
            }],
        })
        .unwrap(),
    );
    let backend_config = serde_yaml::to_string(&underwrap_backendd::BackendConfig {
        stores_dir,
        clients_dir,
        users_dir: base.join("users"),
        bootstrap_file: base.join("bootstrap.json"),
        https: Some(underwrap_backendd::HttpsConfig {
            listen: format!("127.0.0.1:{https_port}"),
            tls: underwrap_backendd::HttpsTlsConfig {
                cert_file: server_cert.clone(),
                key_file: server_key.clone(),
            },
        }),
        admin: Some(underwrap_backendd::AdminConfig {
            allowed_origins: Vec::new(),
            session_lifetime_secs: underwrap_backendd::DEFAULT_SESSION_LIFETIME_SECS,
            stores_data_root: None,
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
    let crate::harness::AdminCredentials {
        session_cookie: admin_session_cookie,
        csrf_cookie: admin_csrf_cookie,
        csrf_token: admin_csrf_token,
    } = bootstrap_admin(&admin_url, &ca_cert, "tester", "hunter2-long")
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
        instance: "local".to_string(),
        socket: wrapper_sock.clone(),
        group: crate::harness::current_group_name(),
        ns_binary: base.join("underwrap-ns"),
        debug_tool_target: base.join("underwrap-debug-tool"),
        xdg_runtime_dir_base: "/run/user".to_string(),
        wrappers: {
            let mut m = HashMap::new();
            let echo = which_echo().unwrap_or_else(|| "/bin/true".to_string());
            m.insert("w".to_string(), wrapper(&echo, "s/t", "T"));
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
    if !wrapper_sock.exists() {
        // Surface the daemon's own diagnostics — otherwise a startup
        // refusal (e.g. a failed validation) is invisible in CI logs.
        let _ = child.start_kill();
        let out = child.wait_with_output().await.unwrap();
        panic!(
            "wrapper socket never appeared\n--- daemon stdout ---\n{}\n--- daemon stderr ---\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }

    // Admin writes are unconditional now that per-resource write
    // flags only gate wrapper-side writes — drop the secret in
    // directly.
    let write_resp = client
        .put(format!(
            "{admin_url}/v1/admin/stores/test/secrets/s%2Ft/env/TEST_TOKEN"
        ))
        .header(
            reqwest::header::COOKIE,
            csrf_cookie_header(&admin_session_cookie, &admin_csrf_cookie),
        )
        .header("X-CSRF-Token", &admin_csrf_token)
        .json(&underwrap_protocol::service::AdminSecretWriteRequest {
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
        .put(format!(
            "{admin_url}/v1/admin/stores/test/wrapper-permissions/inst_underwrap_local/w/s%2Ft",
        ))
        .header(
            reqwest::header::COOKIE,
            csrf_cookie_header(&admin_session_cookie, &admin_csrf_cookie),
        )
        .header("X-CSRF-Token", &admin_csrf_token)
        .json(
            &underwrap_protocol::service::AdminWrapperPermissionUpsertRequest {
                mode: underwrap_protocol::policy::WrapperApprovalMode::Unlocked,
                writable: false,
                reason: None,
            },
        )
        .send()
        .await
        .unwrap();
    assert!(policy_resp.status().is_success());

    let mut stream = tokio::net::UnixStream::connect(&wrapper_sock)
        .await
        .unwrap();
    let req = WrapperRequest::Read {
        wrapper_name: "w".to_string(),
        variant: underwrap_protocol::variant::WrapperVariant::Real,
        scope: None,
    };
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
