use super::*;

#[tokio::test]
async fn parse_example_config() {
    // Inline a representative config matching the structure of examples/config.yaml.
    // This avoids depending on the file being present in nix sandbox builds.
    let text = r#"
credentials:
  github/token:
    mode: env
    variable: GH_TOKEN
  slack/xoxp:
    mode: env
    variable: SLACK_TOKEN
  github/gh:
    mode: libsecret
wrappers:
  gh-authorized:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-authorized.sock
    group: underwrap-gh-authorized
    use:
      github/token:
        access: read-only
  agent-slack:
    target: /usr/bin/slack-cli
    socket: /run/underwrap/agent-slack.sock
    group: underwrap-agent-slack
    use:
      slack/xoxp:
        access: read-only
  gh-libsecret:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-libsecret.sock
    group: underwrap-gh-libsecret
    use:
      github/gh:
        access: read-write
"#;
    let config: underwrap_protocol::config::DaemonConfig = serde_yaml::from_str(text).unwrap();

    assert_eq!(config.wrappers.len(), 3);
    assert_eq!(config.credentials.len(), 3);
    assert!(config.wrappers.contains_key("gh-authorized"));
    assert!(config.wrappers.contains_key("agent-slack"));
    assert!(config.wrappers.contains_key("gh-libsecret"));

    let gh = &config.wrappers["gh-authorized"];
    assert!(gh.use_map.contains_key("github/token"));
    match &config.credentials["github/token"].mode {
        underwrap_protocol::config::CredentialMode::Env { variable } => {
            assert_eq!(variable, "GH_TOKEN");
        }
        other => panic!("expected env credential, got {other:?}"),
    }
}

// ===========================================================================
// underwrapd binary: starts and responds to status
// ===========================================================================

#[tokio::test]
async fn underwrapd_binary_start_and_status() {
    // Ensure the standalone binary is freshly built. `cargo test` may compile
    // the crate without updating `target/debug/underwrapd`, and this test wants
    // to exercise the real binary entry point.
    ensure_package_built("underwrapd");
    ensure_package_built("underwrap-backendd");

    let daemon_bin = require_bin!("underwrapd");

    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();

    let wrapper_sock = base.join("wrapper-w.sock");
    let control_sock = base.join("control.sock");
    let backend_socket = base.join("backend.sock");

    let backend_config = serde_yaml::to_string(&underwrap_backendd::server::BackendConfig {
        storage: underwrap_protocol::config::StorageConfig::Plain {
            dir: base.join("secrets"),
        },
        bindings: HashMap::from([(
            "test".to_string(),
            underwrap_backendd::server::BackendBindingConfig {
                socket: backend_socket.clone(),
                prefix: None,
            },
        )]),
    })
    .unwrap();
    let backend_config_path = base.join("backend.yaml");
    std::fs::write(&backend_config_path, &backend_config).unwrap();

    let backend_bin = require_bin!("underwrap-backendd");
    let mut backend_child = tokio::process::Command::new(&backend_bin)
        .args(["--config", backend_config_path.to_str().unwrap()])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    let config = serde_yaml::to_string(&underwrap_protocol::config::DaemonConfig {
        sockets: underwrap_protocol::config::SocketConfig {
            control: control_sock.clone(),
            ..Default::default()
        },
        backend: underwrap_protocol::config::BackendSocketConfig {
            socket: backend_socket.clone(),
        },
        allowed_users: Vec::new(),
        credentials: {
            let mut m = HashMap::new();
            m.insert("s/t".to_string(), secret(false));
            m
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

    // Wait for sockets.
    for _ in 0..100 {
        if control_sock.exists() && wrapper_sock.exists() && backend_socket.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(control_sock.exists(), "control socket never appeared");
    assert!(wrapper_sock.exists(), "wrapper socket never appeared");

    // Send a status request directly.
    let mut stream = tokio::net::UnixStream::connect(&control_sock)
        .await
        .unwrap();
    let req = ControlRequest::StatusAll;
    underwrap_protocol::ipc::write_frame_async(&mut stream, &req)
        .await
        .unwrap();
    let resp: ControlResponse = underwrap_protocol::ipc::read_frame_async(&mut stream)
        .await
        .unwrap();

    match resp {
        ControlResponse::StatusAllOk { wrappers, .. } => {
            assert_eq!(wrappers.len(), 1);
        }
        other => panic!("expected StatusAllOk, got {other:?}"),
    }

    // Kill the daemon.
    child.kill().await.unwrap();
    backend_child.kill().await.unwrap();
}
