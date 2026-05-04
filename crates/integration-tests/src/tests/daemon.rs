use super::*;
use base64::Engine;

fn pick_unused_port_local() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

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
    resources:
      github/token:
        env: true
  agent-slack:
    target: /usr/bin/slack-cli
    socket: /run/underwrap/agent-slack.sock
    group: underwrap-agent-slack
    resources:
      slack/xoxp:
        env: true
  gh-libsecret:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-libsecret.sock
    group: underwrap-gh-libsecret
    resources:
      github/gh:
        libsecret: true
"#;
    let config: underwrap_protocol::config::DaemonConfig = serde_yaml::from_str(text).unwrap();

    assert_eq!(config.wrappers.len(), 3);
    assert!(config.wrappers.contains_key("gh-authorized"));
    assert!(config.wrappers.contains_key("agent-slack"));
    assert!(config.wrappers.contains_key("gh-libsecret"));

    let gh = &config.wrappers["gh-authorized"];
    assert!(gh.resources["github/token"].env);
}

// ===========================================================================
// underwrapd binary: starts and can read via wrapper using backend admin-seeded state
// ===========================================================================

#[tokio::test]
async fn underwrapd_binary_start_and_read() {
    ensure_package_built("frontend-underwrapd");
    ensure_package_built("backend-backendd");

    let daemon_bin = require_bin!("underwrapd");
    let backend_bin = require_bin!("underwrap-backendd");
    let admin_port = pick_unused_port_local();
    let service_port = pick_unused_port_local();
    let admin_url = format!("http://127.0.0.1:{admin_port}");
    let service_url = format!("https://127.0.0.1:{service_port}");

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

    let backend_config = serde_yaml::to_string(&underwrap_backendd::server::BackendConfig {
        stores: HashMap::from([(
            "test".to_string(),
            underwrap_protocol::config::StorageConfig::Plain {
                dir: base.join("secrets"),
            },
        )]),
        service: Some(underwrap_backendd::server::ServiceConfig {
            listen: format!("127.0.0.1:{service_port}"),
            tls: underwrap_backendd::server::ServiceTlsConfig {
                cert_file: server_cert.clone(),
                key_file: server_key.clone(),
                client_ca_file: ca_cert.clone(),
            },
            clients: HashMap::from([(
                "underwrap-local".to_string(),
                underwrap_backendd::server::ServiceClientConfig {
                    store: "test".to_string(),
                },
            )]),
        }),
        admin: Some(underwrap_backendd::server::AdminConfig {
            listen: format!("127.0.0.1:{admin_port}"),
            tokens: HashMap::from([(
                "tester".to_string(),
                underwrap_backendd::server::AdminTokenConfig {
                    token_file: {
                        let token_path = base.join("admin-token");
                        std::fs::write(&token_path, b"binary-test-token").unwrap();
                        token_path
                    },
                },
            )]),
        }),
    })
    .unwrap();
    let admin_token = "binary-test-token";
    let backend_config_path = base.join("backend.yaml");
    std::fs::write(&backend_config_path, &backend_config).unwrap();

    let mut backend_child = tokio::process::Command::new(&backend_bin)
        .args(["--config", backend_config_path.to_str().unwrap()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let client = reqwest::Client::new();
    let mut backend_ready = false;
    for _ in 0..100 {
        if let Ok(resp) = client
            .get(format!("{admin_url}/v1/admin/stores"))
            .bearer_auth(admin_token)
            .send()
            .await
            && resp.status().is_success()
        {
            backend_ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(backend_ready, "backend admin listener never became ready");

    let config = serde_yaml::to_string(&underwrap_protocol::config::DaemonConfig {
        backend: underwrap_protocol::config::BackendServiceConfig {
            url: service_url.clone(),
            ca_file: Some(ca_cert.clone()),
            client_cert_file: Some(client_cert.clone()),
            client_key_file: Some(client_key.clone()),
        },
        catalog_host: "binary-test".to_string(),
        allowed_users: Vec::new(),
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

    let write_resp = client
        .post(format!("{admin_url}/v1/admin/secret/write"))
        .bearer_auth(admin_token)
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

    let policy_resp = client
        .post(format!("{admin_url}/v1/admin/policy/put"))
        .bearer_auth(admin_token)
        .json(&underwrap_protocol::service::AdminPolicyPutRequest {
            store: "test".to_string(),
            document: underwrap_protocol::policy::PolicyDocument {
                wrappers: Vec::new(),
                rules: vec![underwrap_protocol::policy::PolicyRule {
                    id: "allow-read".to_string(),
                    wrapper: "w".to_string(),
                    resource: "env:s/t".to_string(),
                    operation: "env.read".to_string(),
                    effect: underwrap_protocol::policy::PolicyEffect::Allow,
                }],
                grants: Vec::new(),
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
    let resp: WrapperResponse = underwrap_protocol::ipc::read_frame_async(&mut stream)
        .await
        .unwrap();

    assert_eq!(expect_single_env_string(resp), "value");

    child.kill().await.unwrap();
    backend_child.kill().await.unwrap();
}
