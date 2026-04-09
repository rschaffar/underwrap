//! Test harness: spins up an underwrapd instance with temp directories
//! and provides helpers for IPC.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::UnixStream;
use tokio::sync::Notify;

use underwrap_protocol::config::*;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::*;

/// A running test daemon with temp dirs and sockets.
#[allow(dead_code)]
pub struct TestDaemon {
    pub wrapper_sockets: HashMap<String, PathBuf>,
    pub control_socket: PathBuf,
    pub storage_dir: PathBuf,
    shutdown: Arc<Notify>,
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
        let storage_dir = base.join("secrets");
        let mut wrapper_sockets = HashMap::new();

        for name in wrappers.keys().cloned().collect::<Vec<_>>() {
            let socket = base.join(format!("wrapper-{name}.sock"));
            wrappers.get_mut(&name).expect("wrapper must exist").socket = socket.clone();
            wrapper_sockets.insert(name, socket);
        }

        let config = DaemonConfig {
            sockets: SocketConfig {
                control: control_socket.clone(),
                ..Default::default()
            },
            storage: StorageConfig::Plain {
                dir: storage_dir.clone(),
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

        // Wait for sockets to appear.
        for _ in 0..100 {
            if control_socket.exists() && wrapper_sockets.values().all(|p| p.exists()) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        Ok(Self {
            wrapper_sockets,
            control_socket,
            storage_dir,
            shutdown,
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

    /// Shut down the daemon.
    pub async fn stop(self) -> Result<()> {
        self.shutdown.notify_one();
        self.handle.await??;
        Ok(())
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
