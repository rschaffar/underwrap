//! Test harness: spins up an underwrapd instance with temp directories
//! and provides helpers for IPC.

use anyhow::{Context, Result};
use base64::Engine;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use tokio::net::UnixStream;
use tokio::sync::Notify;

use underwrap_protocol::config::*;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::*;
use underwrap_protocol::policy::{PolicyDocument, PolicyGrant};
use underwrap_protocol::service::CatalogEntry;

mod builders;
mod pki;
mod readiness;

pub use builders::{files_resource, wrapper};
pub(crate) use pki::{
    TEST_CA_PEM, TEST_CLIENT_LOCAL_KEY, TEST_CLIENT_LOCAL_PEM, TEST_SERVER_KEY, TEST_SERVER_PEM,
};

use pki::{principal_client_name, test_pki_paths};
use readiness::{pick_unused_port, wait_for_admin, wait_for_path, wait_for_service};

/// A running test daemon with temp dirs and sockets.
#[allow(dead_code)]
pub struct TestDaemon {
    pub wrapper_sockets: HashMap<String, PathBuf>,
    pub admin_url: String,
    pub admin_token: String,
    pub storage_dir: PathBuf,
    shutdown: Arc<Notify>,
    backend_shutdown: Arc<Notify>,
    backend_handle: thread::JoinHandle<Result<()>>,
    _tempdir: tempfile::TempDir,
    handle: tokio::task::JoinHandle<Result<()>>,
}

fn env_secret_id(resource_id: &str) -> String {
    if resource_id.contains("/env/")
        || resource_id == underwrap_protocol::policy::POLICY_DOCUMENT_SECRET_ID
    {
        resource_id.to_string()
    } else {
        format!("{resource_id}/env/TEST_TOKEN")
    }
}

impl TestDaemon {
    /// Start a daemon with the given wrapper definitions.
    pub async fn start(wrappers: HashMap<String, WrapperDef>) -> Result<Self> {
        Self::start_service_mode(wrappers, "local", "underwrap-local").await
    }

    /// Start a daemon against backend HTTPS service mode.
    pub async fn start_service_mode(
        mut wrappers: HashMap<String, WrapperDef>,
        client_name: &str,
        accepted_principal: &str,
    ) -> Result<Self> {
        let tempdir = tempfile::TempDir::new()?;
        let base = tempdir.path();

        let storage_dir = base.join("secrets");
        let mut wrapper_sockets = HashMap::new();

        for name in wrappers.keys().cloned().collect::<Vec<_>>() {
            let socket = base.join(format!("wrapper-{name}.sock"));
            wrappers.get_mut(&name).expect("wrapper must exist").socket = socket.clone();
            wrapper_sockets.insert(name, socket);
        }

        let service_port = pick_unused_port()?;
        let service_url = format!("https://127.0.0.1:{service_port}");
        let pki = test_pki_paths(base, client_name)?;
        let ready_pki = test_pki_paths(base, principal_client_name(accepted_principal)?)?;

        let admin_port = pick_unused_port()?;
        let admin_url = format!("http://127.0.0.1:{admin_port}");
        let admin_token = format!("test-admin-token-{admin_port}-do-not-reuse-zA=");
        let token_file = base.join("admin-token");
        std::fs::write(&token_file, admin_token.as_bytes())?;
        // 230 moved stores out of the inline config and into a
        // `stores.d/` directory union. The harness writes a single
        // `test.yaml` under a temp dynamic dir so the rest of the
        // setup looks the same to the test code.
        let stores_dir = base.join("stores.d");
        std::fs::create_dir_all(&stores_dir)?;
        let test_store_def = underwrap_protocol::config::StoreDefinition {
            description: None,
            created_at: None,
            storage: StorageConfig::Plain {
                dir: storage_dir.clone(),
            },
        };
        std::fs::write(
            stores_dir.join("test.yaml"),
            serde_yaml::to_string(&test_store_def)?,
        )?;
        let backend_config = underwrap_backendd::server::BackendConfig {
            stores_declarative_dir: None,
            stores_dynamic_dir: Some(stores_dir),
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
            admin: Some(underwrap_backendd::server::AdminConfig {
                listen: format!("127.0.0.1:{admin_port}"),
                tokens: HashMap::from([(
                    "tester".to_string(),
                    underwrap_backendd::server::AdminTokenConfig { token_file },
                )]),
                tls: None,
                allowed_origins: None,
                admin_home_store: None,
                session_key_file: None,
                session_lifetime_secs: None,
                stores_data_roots: None,
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

        wait_for_admin(&admin_url, &admin_token).await?;
        wait_for_service(
            &service_url,
            &ready_pki.ca_cert,
            &ready_pki.client_cert,
            &ready_pki.client_key,
        )
        .await?;

        let config = DaemonConfig {
            backend: BackendServiceConfig {
                url: service_url,
                ca_file: Some(pki.ca_cert.clone()),
                client_cert_file: Some(pki.client_cert.clone()),
                client_key_file: Some(pki.client_key.clone()),
            },
            catalog_host: client_name.to_string(),
            catalog_instance: String::new(),
            allowed_users: Vec::new(),
            wrappers,
        };

        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = Arc::clone(&shutdown);
        let handle =
            tokio::spawn(
                async move { underwrapd::server::run(config, Some(shutdown_clone)).await },
            );

        for socket in wrapper_sockets.values() {
            wait_for_path(socket).await?;
        }

        Ok(Self {
            wrapper_sockets,
            admin_url,
            admin_token,
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

    fn admin_request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        reqwest::Client::new()
            .request(method, format!("{}{}", self.admin_url, path))
            .bearer_auth(&self.admin_token)
    }

    /// Write a secret through the backend admin API.
    ///
    /// Tests typically run against a fresh store with no policy document, so
    /// the admin write gate auto-allows. The `wrapper` parameter is required
    /// by the request shape but is only consulted when a policy document
    /// exists; pass any wrapper name from the test fixture.
    pub async fn store_secret(
        &self,
        store: &str,
        wrapper: &str,
        secret_id: &str,
        material: &str,
    ) -> Result<()> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/secret/write")
            .json(&underwrap_protocol::service::AdminSecretWriteRequest {
                store: store.to_string(),
                wrapper: wrapper.to_string(),
                secret_id: env_secret_id(secret_id),
                material_b64: base64::engine::general_purpose::STANDARD.encode(material.as_bytes()),
            })
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        Ok(())
    }

    pub async fn get_catalog(&self, store: &str) -> Result<Vec<CatalogEntry>> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/catalog/get")
            .json(&underwrap_protocol::service::AdminCatalogGetRequest {
                store: store.to_string(),
            })
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        let body: underwrap_protocol::service::AdminCatalogGetResponse = response.json().await?;
        Ok(body.entries)
    }

    pub async fn get_policy(&self, store: &str) -> Result<Option<PolicyDocument>> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/policy/get")
            .json(&underwrap_protocol::service::AdminPolicyGetRequest {
                store: store.to_string(),
            })
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        let body: underwrap_protocol::service::AdminPolicyGetResponse = response.json().await?;
        Ok(Some(body.document))
    }

    pub async fn put_policy(&self, store: &str, doc: &PolicyDocument) -> Result<()> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/policy/put")
            .json(&underwrap_protocol::service::AdminPolicyPutRequest {
                store: store.to_string(),
                document: doc.clone(),
            })
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        Ok(())
    }

    pub async fn upsert_grant(&self, store: &str, grant: &PolicyGrant) -> Result<()> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/grant/upsert")
            .json(&underwrap_protocol::service::AdminGrantUpsertRequest {
                store: store.to_string(),
                grant: grant.clone(),
            })
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        Ok(())
    }

    pub async fn delete_grant(&self, store: &str, id: &str) -> Result<()> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/grant/delete")
            .json(&underwrap_protocol::service::AdminGrantDeleteRequest {
                store: store.to_string(),
                id: id.to_string(),
            })
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        Ok(())
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
