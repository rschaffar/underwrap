//! Test harness: spins up an underwrapd instance with temp directories
//! and provides helpers for IPC.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use tokio::net::UnixStream;
use tokio::sync::Notify;

use underwrap_protocol::config::*;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::*;
use underwrap_protocol::policy::PolicyDocument;

/// A running test daemon with temp dirs and sockets.
#[allow(dead_code)]
pub struct TestDaemon {
    pub wrapper_sockets: HashMap<String, PathBuf>,
    pub control_socket: PathBuf,
    pub backend_socket: PathBuf,
    pub backend_management_socket: PathBuf,
    pub storage_dir: PathBuf,
    shutdown: Arc<Notify>,
    backend_shutdown: Arc<Notify>,
    backend_handle: thread::JoinHandle<Result<()>>,
    _tempdir: tempfile::TempDir,
    handle: tokio::task::JoinHandle<Result<()>>,
}

impl TestDaemon {
    /// Start a daemon with the given wrapper and credential definitions.
    pub async fn start(
        mut wrappers: HashMap<String, WrapperDef>,
        credentials: HashMap<String, CredentialDef>,
    ) -> Result<Self> {
        let tempdir = tempfile::TempDir::new()?;
        let base = tempdir.path();

        let control_socket = base.join("control.sock");
        let backend_socket = base.join("backend.sock");
        let backend_management_socket = base.join("backend-mgmt.sock");
        let storage_dir = base.join("secrets");
        let mut wrapper_sockets = HashMap::new();

        for name in wrappers.keys().cloned().collect::<Vec<_>>() {
            let socket = base.join(format!("wrapper-{name}.sock"));
            wrappers.get_mut(&name).expect("wrapper must exist").socket = socket.clone();
            wrapper_sockets.insert(name, socket);
        }

        let backend_config = underwrap_backendd::server::BackendConfig {
            management: underwrap_backendd::server::ManagementConfig {
                socket: backend_management_socket.clone(),
            },
            stores: HashMap::from([(
                "test".to_string(),
                StorageConfig::Plain {
                    dir: storage_dir.clone(),
                },
            )]),
            service: None,
        };
        let backend_shutdown = Arc::new(Notify::new());
        let backend_shutdown_clone = Arc::clone(&backend_shutdown);
        let backend_handle = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                underwrap_backendd::server::run(backend_config, Some(backend_shutdown_clone)).await
            })
        });

        wait_for_path(&backend_management_socket).await?;
        backend_export_socket(&backend_management_socket, "test", &backend_socket, 0o600).await?;
        wait_for_path(&backend_socket).await?;

        let config = DaemonConfig {
            sockets: SocketConfig {
                control: control_socket.clone(),
                ..Default::default()
            },
            backend: BackendSocketConfig {
                socket: backend_socket.clone(),
                ..Default::default()
            },
            allowed_users: Vec::new(),
            credentials,
            wrappers,
        };

        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = Arc::clone(&shutdown);

        let handle =
            tokio::spawn(
                async move { underwrapd::server::run(config, Some(shutdown_clone)).await },
            );

        wait_for_path(&control_socket).await?;
        for socket in wrapper_sockets.values() {
            wait_for_path(socket).await?;
        }

        Ok(Self {
            wrapper_sockets,
            control_socket,
            backend_socket,
            backend_management_socket,
            storage_dir,
            shutdown,
            backend_shutdown,
            backend_handle,
            _tempdir: tempdir,
            handle,
        })
    }

    /// Start a daemon against backend HTTPS service mode.
    pub async fn start_service_mode(
        mut wrappers: HashMap<String, WrapperDef>,
        credentials: HashMap<String, CredentialDef>,
        client_name: &str,
        accepted_principal: &str,
    ) -> Result<Self> {
        let tempdir = tempfile::TempDir::new()?;
        let base = tempdir.path();

        let control_socket = base.join("control.sock");
        let backend_socket = base.join("backend.sock");
        let backend_management_socket = base.join("backend-mgmt.sock");
        let storage_dir = base.join("secrets");
        let mut wrapper_sockets = HashMap::new();

        for name in wrappers.keys().cloned().collect::<Vec<_>>() {
            let socket = base.join(format!("wrapper-{name}.sock"));
            wrappers.get_mut(&name).expect("wrapper must exist").socket = socket.clone();
            wrapper_sockets.insert(name, socket);
        }

        let service_port = pick_unused_port()?;
        let service_url = format!("https://127.0.0.1:{service_port}");
        let pki = test_pki_paths(client_name)?;
        let ready_pki = test_pki_paths(principal_client_name(accepted_principal)?)?;

        let backend_config = underwrap_backendd::server::BackendConfig {
            management: underwrap_backendd::server::ManagementConfig {
                socket: backend_management_socket.clone(),
            },
            stores: HashMap::from([(
                "test".to_string(),
                StorageConfig::Plain {
                    dir: storage_dir.clone(),
                },
            )]),
            service: Some(underwrap_backendd::server::ServiceConfig {
                listen: format!("127.0.0.1:{service_port}"),
                tls: underwrap_backendd::server::ServiceTlsConfig {
                    cert_file: pki.server_cert.clone(),
                    key_file: pki.server_key.clone(),
                    client_ca_file: pki.ca_cert.clone(),
                },
                clients: HashMap::from([(
                    accepted_principal.to_string(),
                    underwrap_backendd::server::ServiceClientConfig {
                        store: "test".to_string(),
                    },
                )]),
            }),
        };
        let backend_shutdown = Arc::new(Notify::new());
        let backend_shutdown_clone = Arc::clone(&backend_shutdown);
        let backend_handle = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                underwrap_backendd::server::run(backend_config, Some(backend_shutdown_clone)).await
            })
        });

        wait_for_path(&backend_management_socket).await?;
        wait_for_service(
            &service_url,
            &ready_pki.ca_cert,
            &ready_pki.client_cert,
            &ready_pki.client_key,
        )
        .await?;

        let config = DaemonConfig {
            sockets: SocketConfig {
                control: control_socket.clone(),
                ..Default::default()
            },
            backend: BackendSocketConfig {
                mode: underwrap_protocol::config::BackendMode::Service,
                socket: backend_socket.clone(),
                url: Some(service_url),
                ca_file: Some(pki.ca_cert.clone()),
                client_cert_file: Some(pki.client_cert.clone()),
                client_key_file: Some(pki.client_key.clone()),
            },
            allowed_users: Vec::new(),
            credentials,
            wrappers,
        };

        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = Arc::clone(&shutdown);
        let handle =
            tokio::spawn(
                async move { underwrapd::server::run(config, Some(shutdown_clone)).await },
            );

        wait_for_path(&control_socket).await?;
        for socket in wrapper_sockets.values() {
            wait_for_path(socket).await?;
        }

        Ok(Self {
            wrapper_sockets,
            control_socket,
            backend_socket,
            backend_management_socket,
            storage_dir,
            shutdown,
            backend_shutdown,
            backend_handle,
            _tempdir: tempdir,
            handle,
        })
    }

    fn wrapper_socket(&self, wrapper: &str) -> Result<&PathBuf> {
        self.wrapper_sockets
            .get(wrapper)
            .with_context(|| format!("unknown wrapper socket for '{wrapper}'"))
    }

    /// Send a read request and return the response.
    pub async fn read_request(&self, wrapper: &str) -> Result<WrapperResponse> {
        let mut stream = UnixStream::connect(self.wrapper_socket(wrapper)?).await?;
        let req = WrapperRequest::Read { scope: None };
        write_frame_async(&mut stream, &req).await?;
        let resp: WrapperResponse = read_frame_async(&mut stream).await?;
        Ok(resp)
    }

    /// Get a raw connection to a wrapper socket for multi-message tests.
    pub async fn connect_wrapper(&self, wrapper: &str) -> Result<UnixStream> {
        Ok(UnixStream::connect(self.wrapper_socket(wrapper)?).await?)
    }

    /// Send a read request with a scope.
    #[allow(dead_code)]
    pub async fn read_request_with_scope(
        &self,
        wrapper: &str,
        scope: &str,
    ) -> Result<WrapperResponse> {
        let mut stream = UnixStream::connect(self.wrapper_socket(wrapper)?).await?;
        let req = WrapperRequest::Read {
            scope: Some(scope.to_string()),
        };
        write_frame_async(&mut stream, &req).await?;
        let resp: WrapperResponse = read_frame_async(&mut stream).await?;
        Ok(resp)
    }

    /// Send a control request and return the response.
    pub async fn control_request(&self, req: &ControlRequest) -> Result<ControlResponse> {
        let mut stream = UnixStream::connect(&self.control_socket).await?;
        write_frame_async(&mut stream, req).await?;
        let resp: ControlResponse = read_frame_async(&mut stream).await?;
        Ok(resp)
    }

    /// Store a credential via the control socket.
    pub async fn store_secret(&self, secret_id: &str, material: &str) -> Result<ControlResponse> {
        self.control_request(&ControlRequest::Store {
            secret_id: secret_id.to_string(),
            material: material.to_string().into(),
        })
        .await
    }

    /// Query status via the control socket.
    pub async fn status(&self, target: Option<&str>) -> Result<ControlResponse> {
        let request = match target {
            Some(target) => ControlRequest::StatusTarget {
                target: target.to_string(),
            },
            None => ControlRequest::StatusAll,
        };
        self.control_request(&request).await
    }

    pub async fn get_policy(&self, store: &str) -> Result<Option<PolicyDocument>> {
        let mut stream = UnixStream::connect(&self.backend_management_socket).await?;
        write_frame_async(
            &mut stream,
            &BackendMgmtRequest::GetPolicy {
                store: store.to_string(),
            },
        )
        .await?;
        let response: BackendMgmtResponse = read_frame_async(&mut stream).await?;
        match response {
            BackendMgmtResponse::GetPolicyOk { document_json } => document_json
                .map(|document_json| serde_json::from_str(&document_json))
                .transpose()
                .map_err(Into::into),
            BackendMgmtResponse::Error { message, .. } => anyhow::bail!(message),
            other => anyhow::bail!("unexpected backend management response: {other:?}"),
        }
    }

    pub async fn put_policy(&self, store: &str, doc: &PolicyDocument) -> Result<()> {
        let mut stream = UnixStream::connect(&self.backend_management_socket).await?;
        write_frame_async(
            &mut stream,
            &BackendMgmtRequest::PutPolicy {
                store: store.to_string(),
                document_json: serde_json::to_string(doc)?,
            },
        )
        .await?;
        let response: BackendMgmtResponse = read_frame_async(&mut stream).await?;
        match response {
            BackendMgmtResponse::PutPolicyOk => Ok(()),
            BackendMgmtResponse::Error { message, .. } => anyhow::bail!(message),
            other => anyhow::bail!("unexpected backend management response: {other:?}"),
        }
    }

    /// Shut down the daemon.
    pub async fn stop(self) -> Result<()> {
        self.shutdown.notify_one();
        self.handle.await??;
        self.backend_shutdown.notify_one();
        self.backend_handle.join().unwrap()?;
        Ok(())
    }
}

struct TestPkiPaths {
    ca_cert: PathBuf,
    server_cert: PathBuf,
    server_key: PathBuf,
    client_cert: PathBuf,
    client_key: PathBuf,
}

fn principal_client_name(principal: &str) -> Result<&'static str> {
    match principal {
        "underwrap-local" => Ok("local"),
        "underwrap-tunnel" => Ok("tunnel"),
        other => anyhow::bail!("unknown test principal '{other}'"),
    }
}

fn test_pki_paths(client_name: &str) -> Result<TestPkiPaths> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    let pki_dir = workspace_root.join("testdata/pki");
    let (client_cert, client_key) = match client_name {
        "local" => (
            pki_dir.join("client-local.pem"),
            pki_dir.join("client-local.key"),
        ),
        "tunnel" => (
            pki_dir.join("client-tunnel.pem"),
            pki_dir.join("client-tunnel.key"),
        ),
        other => anyhow::bail!("unknown test client name '{other}'"),
    };
    Ok(TestPkiPaths {
        ca_cert: pki_dir.join("ca.pem"),
        server_cert: pki_dir.join("server.pem"),
        server_key: pki_dir.join("server.key"),
        client_cert,
        client_key,
    })
}

fn pick_unused_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

async fn wait_for_service(
    url: &str,
    ca_cert: &std::path::Path,
    client_cert: &std::path::Path,
    client_key: &std::path::Path,
) -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let ca_pem = std::fs::read(ca_cert)?;
    let cert_pem = std::fs::read(client_cert)?;
    let key_pem = std::fs::read(client_key)?;
    let mut identity_pem = cert_pem;
    identity_pem.extend_from_slice(&key_pem);
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .add_root_certificate(reqwest::Certificate::from_pem(&ca_pem)?)
        .identity(reqwest::Identity::from_pem(&identity_pem)?)
        .build()?;

    for _ in 0..100 {
        if let Ok(response) = client.get(format!("{url}/v1/status")).send().await
            && response.status().is_success()
        {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    anyhow::bail!("service never became ready at {url}")
}

async fn wait_for_path(path: &std::path::Path) -> Result<()> {
    for _ in 0..100 {
        if path.exists() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    anyhow::bail!("path never appeared: {}", path.display())
}

async fn backend_export_socket(
    management_socket: &std::path::Path,
    store: &str,
    backend_socket: &std::path::Path,
    mode: u32,
) -> Result<()> {
    let mut stream = UnixStream::connect(management_socket).await?;
    let request = BackendMgmtRequest::ExportSocket {
        store: store.to_string(),
        socket: backend_socket.display().to_string(),
        mode,
    };
    write_frame_async(&mut stream, &request).await?;
    let response: BackendMgmtResponse = read_frame_async(&mut stream).await?;
    match response {
        BackendMgmtResponse::ExportSocketOk => Ok(()),
        BackendMgmtResponse::Error { message, .. } => anyhow::bail!(message),
        other => anyhow::bail!("unexpected backend management response: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Builder helpers for common test configs
// ---------------------------------------------------------------------------

/// Get the current process's primary group name (for test wrappers).
fn current_group_name() -> String {
    let gid = unsafe { libc::getegid() };
    let grp = unsafe { libc::getgrgid(gid) };
    if grp.is_null() {
        return gid.to_string();
    }
    let name = unsafe { std::ffi::CStr::from_ptr((*grp).gr_name) };
    name.to_string_lossy().to_string()
}

/// Create a simple wrapper def for testing (no unlock required).
pub fn wrapper(target: &str, secret: &str, _env_var: &str) -> WrapperDef {
    let mut use_map = HashMap::new();
    use_map.insert(
        secret.to_string(),
        CredentialUse {
            access: AccessMode::ReadOnly,
        },
    );
    WrapperDef {
        target: PathBuf::from(target),
        use_map,
        unlock: UnlockPolicy::default(),
        socket: PathBuf::new(),
        group: current_group_name(),
        unlock_key: None,
    }
}

/// Create a wrapper def that requires unlock before reads.
pub fn locked_wrapper(target: &str, secret: &str, _env_var: &str) -> WrapperDef {
    let mut use_map = HashMap::new();
    use_map.insert(
        secret.to_string(),
        CredentialUse {
            access: AccessMode::ReadOnly,
        },
    );
    WrapperDef {
        target: PathBuf::from(target),
        use_map,
        unlock: UnlockPolicy {
            mode: UnlockMode::Always,
            max_seconds: Some(3600),
        },
        socket: PathBuf::new(),
        group: current_group_name(),
        unlock_key: None,
    }
}

/// Create a simple env credential def for testing.
pub fn secret(_mutable: bool) -> CredentialDef {
    CredentialDef {
        mode: CredentialMode::Env {
            variable: "UNDERWRAP_SECRET".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_builder_creates_read_only_usage() {
        let wrapper = wrapper("/usr/bin/gh", "github/token", "GH_TOKEN");
        assert_eq!(wrapper.target, PathBuf::from("/usr/bin/gh"));
        assert_eq!(wrapper.use_map.len(), 1);
        assert_eq!(
            wrapper.use_map.get("github/token").unwrap().access,
            AccessMode::ReadOnly
        );
        assert_eq!(wrapper.unlock.mode, UnlockMode::AlwaysUnlocked);
    }

    #[test]
    fn locked_wrapper_builder_sets_unlock_policy() {
        let wrapper = locked_wrapper("/usr/bin/gh", "github/token", "GH_TOKEN");
        assert_eq!(wrapper.unlock.mode, UnlockMode::Always);
        assert_eq!(wrapper.unlock.max_seconds, Some(3600));
        assert_eq!(
            wrapper.use_map.get("github/token").unwrap().access,
            AccessMode::ReadOnly
        );
    }

    #[test]
    fn secret_builder_creates_env_credential() {
        let credential = secret(false);
        match credential.mode {
            CredentialMode::Env { variable } => assert_eq!(variable, "UNDERWRAP_SECRET"),
            other => panic!("expected env credential, got {other:?}"),
        }
    }
}
