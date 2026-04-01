//! Test harness: spins up an underwrapd instance with temp directories
//! and provides helpers for IPC.

use anyhow::Result;
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
    pub read_socket: PathBuf,
    pub control_socket: PathBuf,
    pub storage_dir: PathBuf,
    pub wrappers_dir: PathBuf,
    shutdown: Arc<Notify>,
    _tempdir: tempfile::TempDir,
    handle: tokio::task::JoinHandle<Result<()>>,
}

impl TestDaemon {
    /// Start a daemon with the given wrapper and secret definitions.
    pub async fn start(
        wrappers: HashMap<String, WrapperDef>,
        secrets: HashMap<String, SecretDef>,
    ) -> Result<Self> {
        let tempdir = tempfile::TempDir::new()?;
        let base = tempdir.path();

        let read_socket = base.join("read.sock");
        let control_socket = base.join("control.sock");
        let storage_dir = base.join("secrets");
        let wrappers_dir = base.join("wrappers");
        std::fs::create_dir_all(&wrappers_dir)?;

        let config = DaemonConfig {
            sockets: SocketConfig {
                read: read_socket.clone(),
                control: control_socket.clone(),
                ..Default::default()
            },
            storage: StorageConfig::Plain {
                dir: storage_dir.clone(),
            },
            wrappers,
            secrets,
        };

        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = Arc::clone(&shutdown);

        let handle =
            tokio::spawn(
                async move { underwrapd::server::run(config, Some(shutdown_clone)).await },
            );

        // Wait for sockets to appear.
        for _ in 0..100 {
            if read_socket.exists() && control_socket.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        Ok(Self {
            read_socket,
            control_socket,
            storage_dir,
            wrappers_dir,
            shutdown,
            _tempdir: tempdir,
            handle,
        })
    }

    /// Send a read request and return the response.
    pub async fn read_request(&self, wrapper: &str) -> Result<ReadResponse> {
        let mut stream = UnixStream::connect(&self.read_socket).await?;
        let req = ReadRequest {
            wrapper: wrapper.to_string(),
            scope: None,
        };
        write_frame_async(&mut stream, &req).await?;
        let resp: ReadResponse = read_frame_async(&mut stream).await?;
        Ok(resp)
    }

    /// Send a read request with a scope.
    #[allow(dead_code)]
    pub async fn read_request_with_scope(
        &self,
        wrapper: &str,
        scope: &str,
    ) -> Result<ReadResponse> {
        let mut stream = UnixStream::connect(&self.read_socket).await?;
        let req = ReadRequest {
            wrapper: wrapper.to_string(),
            scope: Some(scope.to_string()),
        };
        write_frame_async(&mut stream, &req).await?;
        let resp: ReadResponse = read_frame_async(&mut stream).await?;
        Ok(resp)
    }

    /// Send a control request and return the response.
    pub async fn control_request(&self, req: &ControlRequest) -> Result<ControlResponse> {
        let mut stream = UnixStream::connect(&self.control_socket).await?;
        write_frame_async(&mut stream, req).await?;
        let resp: ControlResponse = read_frame_async(&mut stream).await?;
        Ok(resp)
    }

    /// Store a secret via the control socket.
    pub async fn store_secret(&self, secret_id: &str, material: &str) -> Result<ControlResponse> {
        self.control_request(&ControlRequest::Store {
            secret_id: secret_id.to_string(),
            material: material.to_string(),
        })
        .await
    }

    /// Query status via the control socket.
    pub async fn status(&self, target: Option<&str>) -> Result<ControlResponse> {
        self.control_request(&ControlRequest::Status {
            target: target.map(|s| s.to_string()),
        })
        .await
    }

    /// Write a wrapper sidecar YAML config file (for underwrap-exec).
    pub fn write_wrapper_config(
        &self,
        name: &str,
        target: &str,
        env_variable: &str,
    ) -> Result<PathBuf> {
        let path = self.wrappers_dir.join(format!("{name}.yaml"));
        let content = format!(
            "target: {target}\nenv_variable: {env_variable}\nread_socket: {}\n",
            self.read_socket.display()
        );
        std::fs::write(&path, content)?;
        Ok(path)
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

/// Create a simple wrapper def for testing (no unlock required).
pub fn wrapper(target: &str, secret: &str, env_var: &str) -> WrapperDef {
    WrapperDef {
        target: PathBuf::from(target),
        secret: secret.to_string(),
        injection: InjectionMode::Env {
            variable: env_var.to_string(),
        },
        derivation: DerivationMode::None,
        require_unlock: false,
    }
}

/// Create a wrapper def that requires unlock before reads.
pub fn locked_wrapper(target: &str, secret: &str, env_var: &str) -> WrapperDef {
    WrapperDef {
        target: PathBuf::from(target),
        secret: secret.to_string(),
        injection: InjectionMode::Env {
            variable: env_var.to_string(),
        },
        derivation: DerivationMode::None,
        require_unlock: true,
    }
}

/// Create a simple secret def for testing.
pub fn secret(mutable: bool) -> SecretDef {
    SecretDef {
        mutable,
        labels: HashMap::new(),
    }
}
