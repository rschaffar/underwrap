use super::*;
use crate::harness::{pick_unused_port, wait_for_service};
use base64::Engine;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

fn checked_flock(file: &File, operation: libc::c_int) -> std::io::Result<()> {
    // SAFETY: `file` owns a live descriptor for the duration of this call;
    // `flock` takes no pointers and does not transfer descriptor ownership.
    let result = unsafe { libc::flock(file.as_raw_fd(), operation) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn json_file_count(path: &std::path::Path) -> usize {
    match std::fs::read_dir(path) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .count(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => panic!("reading {}: {error}", path.display()),
    }
}

fn provision_operator(
    backend_bin: &str,
    config_path: &std::path::Path,
    username: &str,
    password: &str,
) {
    let mut child = std::process::Command::new(backend_bin)
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "admin",
            "bootstrap-user",
            "--username",
            username,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{password}\n").as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "offline bootstrap-user failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn parse_example_config() {
    // Inline a representative config matching examples/daemon-config.yaml.
    // This avoids depending on the file being present in nix sandbox builds.
    let text = r#"
backend:
  url: https://backend.example.internal:8443
  ca_file: /etc/underwrap/ca.pem
  client_identity_file: /var/lib/underwrap-local/client-local.identity.pem
instance: local
socket: /run/underwrap/local.sock
group: underwrap-local
debug_tool_target: /run/current-system/sw/bin/underwrap-debug-tool
target_runtime_dir_base: /run/user
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
async fn network_bootstrap_cannot_create_operator() {
    let backend_bin = cargo_bin("underwrap-backend");
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
    let backend_config = serde_yaml::to_string(&underwrap_backend::BackendConfig {
        stores_dir: base.join("stores"),
        clients_dir: base.join("clients"),
        users_dir: base.join("users"),
        bootstrap_file: base.join("bootstrap.json"),
        https: Some(underwrap_backend::HttpsConfig {
            listen: format!("127.0.0.1:{https_port}"),
            tls: underwrap_backend::HttpsTlsConfig {
                cert_file: server_cert,
                key_file: server_key,
            },
        }),
        admin: Some(underwrap_backend::AdminConfig {
            allowed_origins: Vec::new(),
            session_lifetime_secs: underwrap_backend::DEFAULT_SESSION_LIFETIME_SECS,
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

    assert_eq!(bootstrap_required, Some(true));

    for origin in [None, Some(admin_url.as_str())] {
        let mut request = client
            .post(format!("{admin_url}/v1/admin/auth/bootstrap"))
            .json(&serde_json::json!({
                "username": "network-attacker",
                "password": "attacker-password"
            }));
        if let Some(origin) = origin {
            request = request.header(reqwest::header::ORIGIN, origin);
        }
        let response = request.send().await.unwrap();
        assert!(
            matches!(
                response.status(),
                reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED
            ),
            "removed bootstrap route returned {}",
            response.status()
        );
    }
    assert_eq!(json_file_count(&base.join("users")), 0);
    assert!(!base.join("bootstrap.json").exists());
    let status = client
        .get(format!("{admin_url}/v1/admin/auth/bootstrap/status"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(status["bootstrap_required"], true);

    let _ = backend_child.start_kill();
    let backend_output = backend_child.wait_with_output().await.unwrap();
    assert!(
        backend_output.status.success() || backend_output.status.code().is_none(),
        "backend shutdown failed\n--- backend stdout ---\n{}\n--- backend stderr ---\n{}",
        String::from_utf8_lossy(&backend_output.stdout),
        String::from_utf8_lossy(&backend_output.stderr),
    );
}

#[test]
fn bootstrap_user_processes_serialize_without_touching_other_state() {
    struct BootstrapChildren {
        children: [Option<std::process::Child>; 2],
    }

    impl BootstrapChildren {
        fn new() -> Self {
            Self {
                children: [None, None],
            }
        }

        fn pids(&self) -> [u32; 2] {
            self.children
                .each_ref()
                .map(|child| child.as_ref().unwrap().id())
        }

        fn wait_for_completion(&mut self, timeout: std::time::Duration) {
            let pids = self.pids();
            let deadline = std::time::Instant::now() + timeout;
            let mut statuses = [None, None];
            loop {
                for (child, status) in self.children.iter_mut().zip(&mut statuses) {
                    if status.is_none() {
                        let child = child.as_mut().unwrap();
                        let pid = child.id();
                        *status = child.try_wait().unwrap_or_else(|error| {
                            panic!("checking bootstrap child pid {pid} completion: {error}")
                        });
                    }
                }
                if statuses.iter().all(Option::is_some) {
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out after {timeout:?} waiting for bootstrap child pids {pids:?} to exit after the flock gate was released; observed statuses: {statuses:?}"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }

        fn into_outputs(mut self) -> [std::process::Output; 2] {
            [
                self.children[0].take().unwrap().wait_with_output().unwrap(),
                self.children[1].take().unwrap().wait_with_output().unwrap(),
            ]
        }
    }

    impl Drop for BootstrapChildren {
        fn drop(&mut self) {
            for child in self.children.iter_mut().filter_map(Option::as_mut) {
                match child.try_wait() {
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => {
                        let _ = child.kill();
                    }
                }
                let _ = child.wait();
            }
        }
    }

    fn is_waiting_flock(line: &str, pid: u32, device: u64, inode: u64) -> bool {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let Some(flock) = fields.iter().position(|field| *field == "FLOCK") else {
            return false;
        };
        // Linux /proc/locks inserts `->` immediately before FLOCK for a blocked
        // request. Relative to FLOCK, the fields are then
        // `ADVISORY WRITE <pid> <hex-major>:<hex-minor>:<decimal-inode>`.
        // Locating FLOCK first avoids relying on absolute columns shifted by `->`.
        if flock == 0
            || fields[flock - 1] != "->"
            || fields.get(flock + 1) != Some(&"ADVISORY")
            || fields.get(flock + 2) != Some(&"WRITE")
            || fields.get(flock + 3).and_then(|field| field.parse().ok()) != Some(pid)
        {
            return false;
        }
        let Some(lock_id) = fields.get(flock + 4) else {
            return false;
        };
        let parts = lock_id.split(':').collect::<Vec<_>>();
        if parts.len() != 3 {
            return false;
        }
        let parsed = (
            u64::from_str_radix(parts[0], 16),
            u64::from_str_radix(parts[1], 16),
            parts[2].parse::<u64>(),
        );
        matches!(
            parsed,
            (Ok(major), Ok(minor), Ok(lock_inode))
                if major == u64::from(libc::major(device))
                    && minor == u64::from(libc::minor(device))
                    && lock_inode == inode
        )
    }

    let backend_bin = cargo_bin("underwrap-backend");
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let stores_dir = base.join("stores");
    let clients_dir = base.join("clients");
    std::fs::create_dir_all(&stores_dir).unwrap();
    std::fs::create_dir_all(&clients_dir).unwrap();
    std::fs::write(stores_dir.join("marker"), b"store-state").unwrap();
    std::fs::write(clients_dir.join("marker"), b"client-state").unwrap();
    let config = underwrap_backend::BackendConfig {
        stores_dir: stores_dir.clone(),
        clients_dir: clients_dir.clone(),
        users_dir: base.join("users"),
        bootstrap_file: base.join("bootstrap.json"),
        https: None,
        admin: None,
    };
    let config_path = base.join("backend.yaml");
    std::fs::write(&config_path, serde_yaml::to_string(&config).unwrap()).unwrap();

    let lock_path = base.join("bootstrap.json.lock");
    let gate = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&lock_path)
        .unwrap();
    checked_flock(&gate, libc::LOCK_EX).unwrap();
    let lock_metadata = gate.metadata().unwrap();

    let mut children = BootstrapChildren::new();
    for (slot, username) in children.children.iter_mut().zip(["alice", "bob"]) {
        *slot = Some(
            std::process::Command::new(&backend_bin)
                .args([
                    "--config",
                    config_path.to_str().unwrap(),
                    "admin",
                    "bootstrap-user",
                    "--username",
                    username,
                ])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    const PASSWORD: &[u8] = b"hunter2-long";
    for child in children.children.iter_mut().filter_map(Option::as_mut) {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(PASSWORD).unwrap();
        stdin.write_all(b"\n").unwrap();
        drop(stdin);
    }

    let child_pids = children.pids();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let locks = loop {
        let locks = std::fs::read_to_string("/proc/locks").unwrap();
        if child_pids.iter().all(|pid| {
            locks
                .lines()
                .any(|line| is_waiting_flock(line, *pid, lock_metadata.dev(), lock_metadata.ino()))
        }) {
            break locks;
        }
        for child in children.children.iter_mut().filter_map(Option::as_mut) {
            assert!(
                child.try_wait().unwrap().is_none(),
                "bootstrap child pid {} exited before waiting on the retained flock inode {}",
                child.id(),
                lock_metadata.ino()
            );
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for bootstrap child pids {child_pids:?} to appear as waiting FLOCK entries for device {}:{} inode {}; relevant /proc/locks entries:\n{}",
            libc::major(lock_metadata.dev()),
            libc::minor(lock_metadata.dev()),
            lock_metadata.ino(),
            locks
                .lines()
                .filter(|line| line.contains("FLOCK"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        std::thread::yield_now();
    };
    assert!(child_pids.iter().all(|pid| {
        locks
            .lines()
            .any(|line| is_waiting_flock(line, *pid, lock_metadata.dev(), lock_metadata.ino()))
    }));
    checked_flock(&gate, libc::LOCK_UN).unwrap();
    drop(gate);

    children.wait_for_completion(std::time::Duration::from_secs(10));
    let outputs = children.into_outputs();
    for (index, output) in outputs.iter().enumerate() {
        for (label, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
            assert!(
                !bytes
                    .windows(PASSWORD.len())
                    .any(|window| window == PASSWORD),
                "stdin credential appeared in bootstrap child {index} {label}"
            );
        }
    }
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1,
        "expected one successful bootstrap; statuses: {:?}",
        outputs.each_ref().map(|output| output.status)
    );
    let loser = outputs
        .iter()
        .find(|output| !output.status.success())
        .unwrap();
    assert!(
        String::from_utf8_lossy(&loser.stderr)
            .contains("bootstrap-user: bootstrap has already been completed"),
        "losing bootstrap did not report authoritative completed state: {}",
        String::from_utf8_lossy(&loser.stderr)
    );
    assert_eq!(json_file_count(&config.users_dir), 1);
    assert!(config.bootstrap_file.exists());
    assert_eq!(
        std::fs::read(stores_dir.join("marker")).unwrap(),
        b"store-state"
    );
    assert_eq!(
        std::fs::read(clients_dir.join("marker")).unwrap(),
        b"client-state"
    );
}

#[test]
fn bootstrap_lock_is_released_when_real_owner_is_killed() {
    let backend_bin = cargo_bin("underwrap-backend");
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let config = underwrap_backend::BackendConfig {
        stores_dir: base.join("stores"),
        clients_dir: base.join("clients"),
        users_dir: base.join("users"),
        bootstrap_file: base.join("bootstrap.json"),
        https: None,
        admin: None,
    };
    let config_path = base.join("backend.yaml");
    std::fs::write(&config_path, serde_yaml::to_string(&config).unwrap()).unwrap();

    let lock_path = base.join("bootstrap.json.lock");
    let gate = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&lock_path)
        .unwrap();
    checked_flock(&gate, libc::LOCK_EX).unwrap();
    let retained_inode = std::fs::metadata(&lock_path).unwrap().ino();

    let mut owner = std::process::Command::new(&backend_bin)
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "admin",
            "bootstrap-user",
            "--username",
            "interrupted",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    const OWNER_PASSWORD: &[u8] = b"holder-password";
    let mut owner_stdin = owner.stdin.take().unwrap();
    owner_stdin.write_all(OWNER_PASSWORD).unwrap();
    owner_stdin.write_all(b"\n").unwrap();
    drop(owner_stdin);
    assert!(owner.try_wait().unwrap().is_none());

    // The real command has consumed its closed stdin and is held behind the
    // gate flock. Its credential must not cross into process metadata.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let cmdline = std::fs::read(format!("/proc/{}/cmdline", owner.id())).unwrap();
    assert!(
        !cmdline
            .windows(OWNER_PASSWORD.len())
            .any(|window| window == OWNER_PASSWORD),
        "stdin credential appeared in owner cmdline"
    );
    match std::fs::read(format!("/proc/{}/environ", owner.id())) {
        Ok(environ) => assert!(
            !environ
                .windows(OWNER_PASSWORD.len())
                .any(|window| window == OWNER_PASSWORD),
            "stdin credential appeared in owner environ"
        ),
        // `underwrap-backend` sets PR_SET_DUMPABLE=0 before parsing the CLI.
        // Normal non-root test processes therefore cannot inspect environ;
        // that denial is a stronger boundary than readable credential absence.
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(error) => panic!("reading owner environ: {error}"),
    }

    let probe = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&lock_path)
        .unwrap();
    drop(gate);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match checked_flock(&probe, libc::LOCK_EX | libc::LOCK_NB) {
            Ok(()) => {
                checked_flock(&probe, libc::LOCK_UN).unwrap();
                assert!(
                    owner.try_wait().unwrap().is_none(),
                    "bootstrap owner exited before its lock was observed"
                );
                assert!(
                    std::time::Instant::now() < deadline,
                    "owner never acquired flock"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("probing bootstrap flock: {error}"),
        }
    }

    owner.kill().unwrap();
    let killed = owner.wait_with_output().unwrap();
    assert!(!killed.status.success());
    for (label, bytes) in [("stdout", killed.stdout), ("stderr", killed.stderr)] {
        assert!(
            !bytes
                .windows(OWNER_PASSWORD.len())
                .any(|window| window == OWNER_PASSWORD),
            "stdin credential appeared in owner {label}"
        );
    }

    checked_flock(&probe, libc::LOCK_EX | libc::LOCK_NB)
        .expect("owner death must release the retained bootstrap flock");
    checked_flock(&probe, libc::LOCK_UN).unwrap();
    assert_eq!(std::fs::metadata(&lock_path).unwrap().ino(), retained_inode);
    assert_eq!(json_file_count(&config.users_dir), 0);
    assert!(!config.bootstrap_file.exists());

    provision_operator(
        &backend_bin,
        &config_path,
        "replacement",
        "replacement-password",
    );
    assert_eq!(std::fs::metadata(&lock_path).unwrap().ino(), retained_inode);
    assert_eq!(json_file_count(&config.users_dir), 1);
    assert!(config.bootstrap_file.exists());
}

#[tokio::test]
async fn underwrapd_binary_start_and_read() {
    let daemon_bin = cargo_bin("underwrapd");
    let backend_bin = cargo_bin("underwrap-backend");
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
    let client_identity = base.join("client.identity.pem");
    std::fs::write(&server_cert, crate::harness::TEST_SERVER_PEM).unwrap();
    std::fs::write(&server_key, crate::harness::TEST_SERVER_KEY).unwrap();
    std::fs::write(&ca_cert, crate::harness::TEST_CA_PEM).unwrap();
    std::fs::write(&client_cert, crate::harness::TEST_CLIENT_LOCAL_PEM).unwrap();
    let mut identity = crate::harness::TEST_CLIENT_LOCAL_PEM.as_bytes().to_vec();
    identity.extend_from_slice(crate::harness::TEST_CLIENT_LOCAL_KEY.as_bytes());
    std::fs::write(&client_identity, identity).unwrap();
    // The daemon refuses to start unless its mTLS identity is 0600 or tighter
    // and its parent directory is 0700 or tighter.
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(base, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&client_identity, std::fs::Permissions::from_mode(0o400)).unwrap();
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
    underwrap_backend::store_registry::save_store_record(&stores_dir, &test_store).unwrap();
    let client_cert_pem = std::fs::read_to_string(&client_cert).unwrap();
    let client_instance = underwrap_backend::runtime_clients::ClientInstanceRecord {
        id: "inst_underwrap_local".to_string(),
        display_name: "underwrap-local".to_string(),
        current_store_id: "test".to_string(),
        status: underwrap_backend::runtime_clients::ClientInstanceStatus::Active,
        last_seen_at: None,
        current_credential: underwrap_backend::runtime_clients::current_credential_from_pem(
            client_cert_pem,
        )
        .unwrap(),
        auto_create_wrapper_assets: false,
    };
    underwrap_backend::runtime_clients::save_client_instance(&clients_dir, &client_instance)
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
    let backend_config = serde_yaml::to_string(&underwrap_backend::BackendConfig {
        stores_dir,
        clients_dir,
        users_dir: base.join("users"),
        bootstrap_file: base.join("bootstrap.json"),
        https: Some(underwrap_backend::HttpsConfig {
            listen: format!("127.0.0.1:{https_port}"),
            tls: underwrap_backend::HttpsTlsConfig {
                cert_file: server_cert.clone(),
                key_file: server_key.clone(),
            },
        }),
        admin: Some(underwrap_backend::AdminConfig {
            allowed_origins: Vec::new(),
            session_lifetime_secs: underwrap_backend::DEFAULT_SESSION_LIFETIME_SECS,
            stores_data_root: None,
        }),
    })
    .unwrap();
    let backend_config_path = base.join("backend.yaml");
    std::fs::write(&backend_config_path, &backend_config).unwrap();
    provision_operator(&backend_bin, &backend_config_path, "tester", "hunter2-long");

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
    } = login_admin(&admin_url, &ca_cert, "tester", "hunter2-long")
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
        wait_for_service(&service_url, &ca_cert, &client_identity),
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
            client_identity_file: Some(client_identity.clone()),
        },
        instance: "local".to_string(),
        socket: wrapper_sock.clone(),
        group: crate::harness::current_group_name(),
        debug_tool_target: base.join("underwrap-debug-tool"),
        target_runtime_dir_base: "/run/user".to_string(),
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
        let _ = backend_child.start_kill();
        let out = child.wait_with_output().await.unwrap();
        let _ = backend_child.wait_with_output().await.unwrap();
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

    let (_, blob, _) = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        checkout_resource(wrapper_sock.clone(), "w"),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            let _ = child.start_kill();
            let _ = backend_child.start_kill();
            let _ = child.wait_with_output().await.unwrap();
            let _ = backend_child.wait_with_output().await.unwrap();
            panic!("wrapper read timed out");
        }
    };

    assert_eq!(expect_single_env_string(blob), "value");

    let child_kill = child.start_kill();
    let backend_kill = backend_child.start_kill();
    let _ = child.wait_with_output().await.unwrap();
    let _ = backend_child.wait_with_output().await.unwrap();
    child_kill.unwrap();
    backend_kill.unwrap();
}
