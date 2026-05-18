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
use underwrap_protocol::service::CatalogEntry;

mod builders;
mod pki;
mod readiness;

pub use builders::{files_resource, wrapper};
pub(crate) use pki::{
    TEST_CA_PEM, TEST_CLIENT_LOCAL_KEY, TEST_CLIENT_LOCAL_PEM, TEST_SERVER_KEY, TEST_SERVER_PEM,
};

use pki::{principal_client_name, test_pki_paths};
pub(crate) use readiness::{pick_unused_port, wait_for_service};
use readiness::{wait_for_admin, wait_for_path};

/// A running test daemon with temp dirs and sockets.
#[allow(dead_code)]
pub struct TestDaemon {
    pub wrapper_sockets: HashMap<String, PathBuf>,
    pub admin_url: String,
    pub admin_token: String,
    pub service_url: String,
    pub storage_dir: PathBuf,
    service_ca_cert: PathBuf,
    service_client_cert: PathBuf,
    service_client_key: PathBuf,
    /// The `catalog_host` configured on the daemon. Tests use it to
    /// build grants for `(wrapper, host, current_user)`.
    catalog_host: String,
    shutdown: Arc<Notify>,
    backend_shutdown: Arc<Notify>,
    backend_handle: thread::JoinHandle<Result<()>>,
    _tempdir: tempfile::TempDir,
    handle: tokio::task::JoinHandle<Result<()>>,
}

/// Resolve the username of the user running `cargo test`. Used to
/// populate the `user` field on agent grants the tests construct.
pub(crate) fn current_username() -> String {
    let uid = unsafe { libc::getuid() };
    // SAFETY: getpwuid returns either null or a pointer to a static
    // passwd struct. We copy the name out immediately.
    let pwd = unsafe { libc::getpwuid(uid) };
    if pwd.is_null() {
        // Fall back to a sentinel if the env has no passwd entry;
        // the test will fail at the grant-mismatch assertion anyway.
        return format!("uid-{uid}");
    }
    let c_name = unsafe { (*pwd).pw_name };
    if c_name.is_null() {
        return format!("uid-{uid}");
    }
    unsafe { std::ffi::CStr::from_ptr(c_name) }
        .to_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|_| format!("uid-{uid}"))
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
            hostname_hint: Some(client_name.to_string()),
            instance_name: client_name.to_string(),
            current_store_id: "test".to_string(),
            status: underwrap_backendd::runtime_clients::ClientInstanceStatus::Active,
            last_seen_at: None,
            current_credential: underwrap_backendd::runtime_clients::current_credential_from_pem(
                ready_cert_pem,
                None,
            )?,
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
            enrollment: None,
            service: Some(underwrap_backendd::server::ServiceConfig {
                listen: format!("127.0.0.1:{service_port}"),
                tls: underwrap_backendd::server::ServiceTlsConfig {
                    cert_file: pki.server_cert.clone(),
                    key_file: pki.server_key.clone(),
                    client_ca_file: pki.ca_cert.clone(),
                },
                clients: HashMap::new(),
            }),
            admin: Some(underwrap_backendd::server::AdminConfig {
                listen: format!("127.0.0.1:{admin_port}"),
                tokens: HashMap::from([(
                    "tester".to_string(),
                    underwrap_backendd::server::AdminTokenConfig { token_file },
                )]),
                tls: None,
                allowed_origins: None,
                session_key_file: None,
                session_lifetime_secs: None,
                stores_data_roots: Some(vec![stores_data_root]),
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
                url: service_url.clone(),
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
            service_url: service_url.clone(),
            storage_dir,
            service_ca_cert: pki.ca_cert.clone(),
            service_client_cert: pki.client_cert.clone(),
            service_client_key: pki.client_key.clone(),
            catalog_host: client_name.to_string(),
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
        // The wrapper-side read gates on an `AgentGrant` matching
        // `(wrapper, host, user)` (ticket 246); admin writes are
        // unconditional now that per-resource write flags only apply
        // to wrapper writes. Seed a grant for the current user so
        // the read flow under test still passes.
        self.put_policy(
            store,
            &PolicyDocument {
                grants: vec![self.current_user_grant(wrapper)],
                ..Default::default()
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
        wrapper: &str,
        host: &str,
        user: &str,
    ) -> Result<()> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/grants/delete")
            .json(&underwrap_protocol::service::AdminAgentGrantDeleteRequest {
                store: store.to_string(),
                wrapper: wrapper.to_string(),
                host: host.to_string(),
                user: user.to_string(),
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

    /// Convenience: build an `Unlocked` grant for the given wrapper on
    /// the harness's `(catalog_host, current-user)` principal. Used by
    /// almost every read-flow test.
    pub fn current_user_grant(&self, wrapper: &str) -> AgentGrant {
        AgentGrant {
            wrapper: wrapper.to_string(),
            host: self.catalog_host().to_string(),
            user: current_username(),
            mode: underwrap_protocol::policy::WrapperApprovalMode::Unlocked,
            reason: None,
        }
    }

    pub fn catalog_host(&self) -> &str {
        &self.catalog_host
    }

    /// Flip the per-resource wrapper-write flag for one of the two
    /// gated slots (files / libsecret). The backend serialises this
    /// onto `PolicyDocument::files_writable` / `libsecret_writable`.
    pub async fn set_write_permission(
        &self,
        store: &str,
        resource: &str,
        slot: underwrap_protocol::service::AdminWritableSlot,
        enabled: bool,
    ) -> Result<()> {
        let response = self
            .admin_request(
                reqwest::Method::POST,
                "/v1/admin/policy/write-permissions/set",
            )
            .json(
                &underwrap_protocol::service::AdminWritePermissionsSetRequest {
                    store: store.to_string(),
                    resource: resource.to_string(),
                    slot,
                    enabled,
                },
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

    /// Shut down the daemon.
    pub async fn stop(self) -> Result<()> {
        self.shutdown.notify_one();
        self.handle.await??;
        self.backend_shutdown.notify_one();
        self.backend_handle.join().unwrap()?;
        Ok(())
    }
}
