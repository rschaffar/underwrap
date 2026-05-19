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
use underwrap_protocol::policy::{AgentGrant, PolicyDocument};

mod builders;
mod pki;
mod readiness;

pub use builders::{files_resource, wrapper};
pub(crate) use pki::{
    TEST_CA_PEM, TEST_CLIENT_LOCAL_KEY, TEST_CLIENT_LOCAL_PEM, TEST_SERVER_KEY, TEST_SERVER_PEM,
};

use pki::{principal_client_name, test_pki_paths};
use readiness::wait_for_path;
pub(crate) use readiness::{
    bootstrap_admin, csrf_cookie_header, pick_unused_port, wait_for_admin, wait_for_service,
};

/// A running test daemon with temp dirs and sockets.
#[allow(dead_code)]
pub struct TestDaemon {
    pub wrapper_sockets: HashMap<String, PathBuf>,
    pub admin_url: String,
    pub service_url: String,
    pub storage_dir: PathBuf,
    admin_session_cookie: String,
    admin_csrf_token: String,
    admin_client: reqwest::Client,
    pub service_ca_cert: PathBuf,
    service_client_cert: PathBuf,
    service_client_key: PathBuf,
    /// The mTLS-bound runtime client instance id the backend
    /// authenticates this daemon as. Tests use it to build per-grant
    /// keys.
    client_instance: String,
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
        wrappers: HashMap<String, WrapperDef>,
        client_name: &str,
        accepted_principal: &str,
    ) -> Result<Self> {
        Self::start_service_mode_with(wrappers, client_name, accepted_principal, false).await
    }

    /// Variant that seeds the client-instance with
    /// `auto_create_wrapper_assets` already flipped on so the startup
    /// register call creates resource/pair records on first contact.
    pub async fn start_service_mode_with(
        mut wrappers: HashMap<String, WrapperDef>,
        client_name: &str,
        accepted_principal: &str,
        auto_create_wrapper_assets: bool,
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

        let https_port = pick_unused_port()?;
        let service_url = format!("https://127.0.0.1:{https_port}");
        let admin_url = service_url.clone();
        let pki = test_pki_paths(base, client_name)?;
        let ready_pki = test_pki_paths(base, principal_client_name(accepted_principal)?)?;

        let session_key_file = base.join("admin-session-key");
        std::fs::write(
            &session_key_file,
            [7u8; underwrap_backendd::auth::SESSION_KEY_LEN],
        )?;
        let stores_file = base.join("stores.json");
        let instances_file = base.join("client-instances.json");
        let stores_data_root = base.join("stores-data-root");
        std::fs::create_dir_all(&stores_data_root)?;
        let keys_dir = base.join("keys");
        std::fs::create_dir_all(&keys_dir)?;
        let test_store = underwrap_protocol::config::StoreRecord {
            id: "test".to_string(),
            name: "test".to_string(),
            description: None,
            created_at: None,
            storage: StorageConfig::Plain {
                dir: storage_dir.clone(),
            },
        };
        std::fs::write(
            &stores_file,
            serde_json::to_vec_pretty(&underwrap_protocol::config::StoreRegistryDocument {
                version: 1,
                stores: vec![test_store],
            })?,
        )?;
        let ready_cert_pem = std::fs::read_to_string(&ready_pki.client_cert)?;
        let runtime_client = underwrap_backendd::runtime_clients::ClientInstanceRecord {
            id: format!("inst_{accepted_principal}"),
            display_name: accepted_principal.to_string(),
            current_store_id: "test".to_string(),
            status: underwrap_backendd::runtime_clients::ClientInstanceStatus::Active,
            last_seen_at: None,
            current_credential: underwrap_backendd::runtime_clients::current_credential_from_pem(
                ready_cert_pem,
                None,
            )?,
            auto_create_wrapper_assets,
        };
        std::fs::write(
            &instances_file,
            serde_json::to_vec_pretty(
                &underwrap_backendd::runtime_clients::ClientInstancesDocument {
                    version: 1,
                    instances: vec![runtime_client],
                },
            )?,
        )?;
        let backend_config = underwrap_backendd::server::BackendConfig {
            stores_file: Some(stores_file),
            client_instances_file: Some(instances_file),
            operators_file: Some(base.join("operators.json")),
            https: Some(underwrap_backendd::server::HttpsConfig {
                listen: format!("127.0.0.1:{https_port}"),
                tls: underwrap_backendd::server::HttpsTlsConfig {
                    cert_file: pki.server_cert.clone(),
                    key_file: pki.server_key.clone(),
                },
            }),
            admin: Some(underwrap_backendd::server::AdminConfig {
                allowed_origins: None,
                session_key_file: session_key_file.clone(),
                session_lifetime_secs: None,
                stores_data_root: Some(stores_data_root),
                keys_dir: Some(keys_dir),
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

        wait_for_admin(&admin_url, &pki.ca_cert).await?;
        let (admin_session_cookie, admin_csrf_token) =
            bootstrap_admin(&admin_url, &pki.ca_cert, "tester", "hunter2").await?;
        let admin_client = reqwest::Client::builder()
            .use_rustls_tls()
            .add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(
                &pki.ca_cert,
            )?)?)
            .build()?;
        wait_for_service(
            &service_url,
            &ready_pki.ca_cert,
            &ready_pki.client_cert,
            &ready_pki.client_key,
        )
        .await?;

        let config = DaemonConfig {
            backend: BackendServiceConfig {
                url: service_url.clone(),
                ca_file: Some(pki.ca_cert.clone()),
                client_cert_file: Some(pki.client_cert.clone()),
                client_key_file: Some(pki.client_key.clone()),
            },
            wrappers,
        };
        let client_instance = format!("inst_{accepted_principal}");

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
            service_url: service_url.clone(),
            storage_dir,
            admin_session_cookie,
            admin_csrf_token,
            admin_client,
            service_ca_cert: pki.ca_cert.clone(),
            service_client_cert: pki.client_cert.clone(),
            service_client_key: pki.client_key.clone(),
            client_instance,
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

    pub(crate) fn admin_request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> reqwest::RequestBuilder {
        let is_state_changing = !matches!(
            method,
            reqwest::Method::GET | reqwest::Method::HEAD | reqwest::Method::OPTIONS
        );
        let builder = self
            .admin_client
            .request(method.clone(), format!("{}{}", self.admin_url, path))
            .header(
                reqwest::header::COOKIE,
                csrf_cookie_header(&self.admin_session_cookie, &self.admin_csrf_token),
            );
        if is_state_changing {
            builder.header("X-CSRF-Token", &self.admin_csrf_token)
        } else {
            builder
        }
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
        // The wrapper-side read gates on an `AgentGrant` matching
        // `(client_instance, wrapper, resource)`; seed a grant for
        // this daemon's instance so the read flow under test still
        // passes. The `secret_id` argument is the resource id.
        self.put_policy(
            store,
            &PolicyDocument {
                grants: vec![self.current_grant(wrapper, secret_id)],
            },
        )
        .await?;
        self.admin_write_raw(store, wrapper, secret_id, material)
            .await
    }

    /// Write a secret through the admin API *without* touching the
    /// policy document first. Used by tests that exercise the admin
    /// write path in isolation.
    pub async fn admin_write_raw(
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

    pub async fn direct_service_status(
        &self,
    ) -> Result<underwrap_protocol::service::ServiceStatusResponse> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let ca_pem = std::fs::read(&self.service_ca_cert)?;
        let cert_pem = std::fs::read(&self.service_client_cert)?;
        let key_pem = std::fs::read(&self.service_client_key)?;
        let mut identity_pem = cert_pem;
        identity_pem.extend_from_slice(&key_pem);
        let client = reqwest::Client::builder()
            .use_rustls_tls()
            .add_root_certificate(reqwest::Certificate::from_pem(&ca_pem)?)
            .identity(reqwest::Identity::from_pem(&identity_pem)?)
            .build()?;
        let response = client
            .get(format!("{}/v1/status", self.service_url))
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "service API returned {}",
            response.status()
        );
        Ok(response.json().await?)
    }

    pub async fn create_store(
        &self,
        body: &underwrap_protocol::service::AdminStoreCreateRequest,
    ) -> Result<underwrap_protocol::service::AdminStoreCreateResponse> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/stores")
            .json(body)
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        Ok(response.json().await?)
    }

    pub async fn list_client_instances(
        &self,
    ) -> Result<underwrap_protocol::service::AdminClientInstancesListResponse> {
        let response = self
            .admin_request(reqwest::Method::GET, "/v1/admin/client-instances")
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        Ok(response.json().await?)
    }

    pub async fn update_client_instance(
        &self,
        id: &str,
        body: &underwrap_protocol::service::AdminClientInstanceUpdateRequest,
    ) -> Result<underwrap_protocol::service::AdminClientInstanceDetailResponse> {
        let response = self
            .admin_request(
                reqwest::Method::PUT,
                &format!("/v1/admin/client-instances/{id}"),
            )
            .json(body)
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        Ok(response.json().await?)
    }

    pub async fn delete_client_instance(&self, id: &str) -> Result<()> {
        let response = self
            .admin_request(
                reqwest::Method::DELETE,
                &format!("/v1/admin/client-instances/{id}"),
            )
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        Ok(())
    }

    pub async fn list_resources(
        &self,
        store: &str,
    ) -> Result<underwrap_protocol::service::AdminResourcesListResponse> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/resources/list")
            .json(&underwrap_protocol::service::AdminResourcesListRequest {
                store: store.to_string(),
            })
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        Ok(response.json().await?)
    }

    pub async fn create_resource(
        &self,
        store: &str,
        name: &str,
    ) -> Result<underwrap_protocol::service::AdminResourceCreateResponse> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/resources/create")
            .json(&underwrap_protocol::service::AdminResourceCreateRequest {
                store: store.to_string(),
                name: name.to_string(),
            })
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}: {}",
            response.status(),
            response.text().await.unwrap_or_default(),
        );
        Ok(response.json().await?)
    }

    pub async fn delete_resource(
        &self,
        store: &str,
        name: &str,
    ) -> Result<underwrap_protocol::service::AdminResourceDeleteResponse> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/resources/delete")
            .json(&underwrap_protocol::service::AdminResourceDeleteRequest {
                store: store.to_string(),
                name: name.to_string(),
            })
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}: {}",
            response.status(),
            response.text().await.unwrap_or_default(),
        );
        Ok(response.json().await?)
    }

    pub async fn list_pairs(
        &self,
        store: &str,
    ) -> Result<underwrap_protocol::service::AdminPairsListResponse> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/pairs/list")
            .json(&underwrap_protocol::service::AdminPairsListRequest {
                store: store.to_string(),
            })
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        Ok(response.json().await?)
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

    pub async fn upsert_grant(&self, store: &str, grant: &AgentGrant) -> Result<()> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/grants/upsert")
            .json(&underwrap_protocol::service::AdminAgentGrantUpsertRequest {
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

    pub async fn delete_grant(
        &self,
        store: &str,
        client_instance: &str,
        wrapper: &str,
        resource: &str,
    ) -> Result<()> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/grants/delete")
            .json(&underwrap_protocol::service::AdminAgentGrantDeleteRequest {
                store: store.to_string(),
                client_instance: client_instance.to_string(),
                wrapper: wrapper.to_string(),
                resource: resource.to_string(),
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

    /// Convenience: build an `Unlocked` grant for the given wrapper +
    /// resource on this harness's mTLS-bound client instance. Used by
    /// almost every read-flow test.
    pub fn current_grant(&self, wrapper: &str, resource: &str) -> AgentGrant {
        AgentGrant {
            client_instance: self.client_instance.clone(),
            wrapper: wrapper.to_string(),
            resource: resource.to_string(),
            mode: underwrap_protocol::policy::WrapperApprovalMode::Unlocked,
            writable: false,
            reason: None,
        }
    }

    pub fn client_instance(&self) -> &str {
        &self.client_instance
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
