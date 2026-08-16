//! Test harness: spins up an underwrapd instance with temp directories
//! and provides helpers for IPC.

use anyhow::{Context, Result};
use base64::Engine;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use tokio::net::UnixStream;
use tokio::sync::Notify;

/// Characters to percent-encode when interpolating a value into a URL
/// path segment. We escape every reserved/delimiter character that
/// would otherwise split the segment.
const PATH_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'?')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'{')
    .add(b'}');

fn pe(s: &str) -> String {
    utf8_percent_encode(s, PATH_SEGMENT).to_string()
}

use underwrap_protocol::config::*;
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::*;
use underwrap_protocol::policy::{PolicyDocument, WrapperPermission};

mod builders;
mod pki;
mod readiness;

pub(crate) use builders::current_group_name;
pub use builders::{files_wrapper, files_wrapper_paths, wrapper};
pub(crate) use pki::{
    TEST_CA_PEM, TEST_CA_SELECTED_EXCLUSIVE_PEM, TEST_CA_UNSELECTED_PEM, TEST_CLIENT_LOCAL_KEY,
    TEST_CLIENT_LOCAL_PEM, TEST_CLIENT_TUNNEL_KEY, TEST_CLIENT_TUNNEL_PEM, TEST_SERVER_KEY,
    TEST_SERVER_PEM, TEST_SERVER_SELECTED_EXCLUSIVE_KEY, TEST_SERVER_SELECTED_EXCLUSIVE_PEM,
    TEST_SERVER_UNSELECTED_CA_KEY, TEST_SERVER_UNSELECTED_CA_PEM, TEST_SERVER_WRONG_HOST_KEY,
    TEST_SERVER_WRONG_HOST_PEM,
};

use pki::{principal_client_name, test_pki_paths};
use readiness::wait_for_path;
pub(crate) use readiness::{
    AdminCredentials, csrf_cookie_header, login_admin, pick_unused_port, wait_for_admin,
    wait_for_service,
};

/// A running test daemon with temp dirs and sockets.
#[allow(dead_code)]
pub struct TestDaemon {
    pub instance_socket: PathBuf,
    pub service_url: String,
    pub storage_dir: PathBuf,
    /// Root directory under which the backend's admin-created stores
    /// live. Used by [`Self::store_dir_for`] to compute the on-disk
    /// path of a store other than the seeded "test" one.
    pub stores_data_root: PathBuf,
    pub(crate) stores_dir: PathBuf,
    pub(crate) clients_dir: PathBuf,
    admin_session_cookie: String,
    admin_csrf_cookie: String,
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

/// Conventional fixture-level secret key. Test fixtures write a
/// single env entry under each resource via [`TestDaemon::store_secret`];
/// callers receive a `(resource, slot, key)` triple internally where
/// `key` is this constant.
const FIXTURE_ENV_KEY: &str = "TEST_TOKEN";

#[derive(Debug, Clone, Copy)]
pub(crate) enum TestRuntimeIdentity {
    Local,
    Tunnel,
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
        wrappers: HashMap<String, WrapperDef>,
        client_name: &str,
        accepted_principal: &str,
        auto_create_wrapper_assets: bool,
    ) -> Result<Self> {
        let tempdir = tempfile::TempDir::new()?;
        let base = tempdir.path();

        let storage_dir = base.join("secrets");

        // One socket per instance. The wrapper name travels in the IPC
        // request and the daemon validates it against the instance config.
        let instance_socket = base.join("instance.sock");

        let https_port = pick_unused_port()?;
        let service_url = format!("https://127.0.0.1:{https_port}");
        let pki = test_pki_paths(base, client_name)?;
        let ready_pki = test_pki_paths(base, principal_client_name(accepted_principal)?)?;

        let stores_dir = base.join("stores");
        let clients_dir = base.join("clients");
        let stores_data_root = base.join("stores-data-root");
        std::fs::create_dir_all(&stores_data_root)?;
        let test_store = underwrap_protocol::config::StoreRecord {
            id: "test".to_string(),
            name: "test".to_string(),
            description: None,
            created_at: None,
            storage: StorageConfig::Plain {
                dir: storage_dir.clone(),
            },
        };
        underwrap_backend::store_registry::save_store_record(&stores_dir, &test_store)?;
        let ready_cert_pem = std::fs::read_to_string(&ready_pki.client_cert)?;
        let runtime_client = underwrap_backend::runtime_clients::ClientInstanceRecord {
            id: format!("inst_{accepted_principal}"),
            display_name: accepted_principal.to_string(),
            current_store_id: "test".to_string(),
            status: underwrap_backend::runtime_clients::ClientInstanceStatus::Active,
            last_seen_at: None,
            current_credential: underwrap_backend::runtime_clients::current_credential_from_pem(
                ready_cert_pem,
            )?,
            auto_create_wrapper_assets,
        };
        underwrap_backend::runtime_clients::save_client_instance(&clients_dir, &runtime_client)?;
        let backend_config = underwrap_backend::BackendConfig {
            stores_dir: stores_dir.clone(),
            clients_dir: clients_dir.clone(),
            users_dir: base.join("users"),
            bootstrap_file: base.join("bootstrap.json"),
            https: Some(underwrap_backend::HttpsConfig {
                listen: format!("127.0.0.1:{https_port}"),
                tls: underwrap_backend::HttpsTlsConfig {
                    cert_file: pki.server_cert.clone(),
                    key_file: pki.server_key.clone(),
                },
            }),
            admin: Some(underwrap_backend::AdminConfig {
                allowed_origins: Vec::new(),
                session_lifetime_secs: underwrap_backend::DEFAULT_SESSION_LIFETIME_SECS,
                stores_data_root: Some(stores_data_root.clone()),
            }),
        };
        underwrap_backend::cli::bootstrap_user(
            &backend_config.users_dir,
            &backend_config.bootstrap_file,
            "tester",
            "hunter2-long",
        )?;

        let backend_shutdown = Arc::new(Notify::new());
        let backend_shutdown_clone = Arc::clone(&backend_shutdown);
        let backend_handle = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                underwrap_backend::server::run(backend_config, Some(backend_shutdown_clone)).await
            })
        });

        wait_for_admin(&service_url, &pki.ca_cert).await?;
        let AdminCredentials {
            session_cookie: admin_session_cookie,
            csrf_cookie: admin_csrf_cookie,
            csrf_token: admin_csrf_token,
        } = login_admin(&service_url, &pki.ca_cert, "tester", "hunter2-long").await?;
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
            instance: "local".to_string(),
            socket: instance_socket.clone(),
            // The instance group is the test user's primary group so the
            // daemon's `egid == instance gid` gate passes for in-process
            // connections.
            group: builders::current_group_name(),
            ns_binary: base.join("underwrap-ns"),
            debug_tool_target: base.join("underwrap-debug-tool"),
            target_runtime_dir_base: "/run/user".to_string(),
            wrappers,
        };
        let client_instance = format!("inst_{accepted_principal}");

        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = Arc::clone(&shutdown);
        let handle =
            tokio::spawn(
                async move { underwrapd::server::run(config, Some(shutdown_clone)).await },
            );

        wait_for_path(&instance_socket).await?;

        Ok(Self {
            instance_socket,
            service_url: service_url.clone(),
            storage_dir,
            stores_data_root,
            stores_dir,
            clients_dir,
            admin_session_cookie,
            admin_csrf_cookie,
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

    /// Send a read request and return the response.
    pub async fn read_request(&self, wrapper: &str) -> Result<WrapperResponse> {
        let mut stream = UnixStream::connect(&self.instance_socket).await?;
        let req = WrapperRequest::Read {
            wrapper_name: wrapper.to_string(),
            variant: underwrap_protocol::variant::WrapperVariant::Real,
            scope: None,
        };
        write_frame_async(&mut stream, &req).await?;
        let resp: WrapperResponse = read_frame_async(&mut stream).await?;
        Ok(resp)
    }

    /// Send a read request with an explicit variant (Real/Debug).
    pub async fn read_request_variant(
        &self,
        wrapper: &str,
        variant: underwrap_protocol::variant::WrapperVariant,
    ) -> Result<WrapperResponse> {
        let mut stream = UnixStream::connect(&self.instance_socket).await?;
        let req = WrapperRequest::Read {
            wrapper_name: wrapper.to_string(),
            variant,
            scope: None,
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
            .request(method.clone(), format!("{}{}", self.service_url, path))
            .header(
                reqwest::header::COOKIE,
                csrf_cookie_header(&self.admin_session_cookie, &self.admin_csrf_cookie),
            );
        if is_state_changing {
            builder.header("X-CSRF-Token", &self.admin_csrf_token)
        } else {
            builder
        }
    }

    /// Write a secret through the backend admin API.
    ///
    /// The admin write path is gated by operator session auth, not by
    /// the wrapper policy; the `wrapper` argument is here so the helper
    /// can also seed an `Unlocked` `WrapperPermission` for the read
    /// flow under test (the daemon-side gate is deny-by-default).
    pub async fn store_secret(
        &self,
        store: &str,
        wrapper: &str,
        secret_id: &str,
        material: &str,
    ) -> Result<()> {
        // Runtime policy resolution requires a registered resource and pair, so
        // seed both before creating the permission. Test fixtures use one string
        // as the stable resource id and operator-facing resource name.
        self.ensure_pair_in_store(store, wrapper, secret_id)?;
        self.put_policy_for_store(store, secret_id, wrapper).await?;
        self.admin_write_raw(store, wrapper, secret_id, material)
            .await
    }

    /// Like `put_policy` but skips the cross-store pair-seeding done
    /// inside `put_policy`. Used by `store_secret` which already
    /// seeded the pair into the target store explicitly.
    async fn put_policy_for_store(
        &self,
        store: &str,
        resource_id: &str,
        wrapper: &str,
    ) -> Result<()> {
        let permission = self.current_permission(wrapper, resource_id);
        self.admin_upsert_permission(store, &permission).await
    }

    /// Low-level wrapper-permissions/upsert call without the cross-store
    /// pair seeding that [`upsert_permission`] does. Used by the
    /// `put_policy*` helpers which seed pairs separately.
    async fn admin_upsert_permission(
        &self,
        store: &str,
        permission: &WrapperPermission,
    ) -> Result<()> {
        let path = format!(
            "/v1/admin/stores/{}/wrapper-permissions/{}/{}/{}",
            pe(store),
            pe(&permission.client_instance),
            pe(&permission.wrapper),
            pe(&permission.resource_id),
        );
        let response = self
            .admin_request(reqwest::Method::PUT, &path)
            .json(
                &underwrap_protocol::service::AdminWrapperPermissionUpsertRequest {
                    mode: permission.mode.clone(),
                    writable: permission.writable,
                    reason: permission.reason.clone(),
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

    /// Compute the on-disk directory backing `store_id`. For the
    /// pre-seeded "test" store this is the `storage_dir` field; for
    /// any other store id (admin-created on the fly) the backend
    /// places it under `stores_data_root/<store_id>/`.
    pub fn store_dir_for(&self, store_id: &str) -> PathBuf {
        if store_id == "test" {
            self.storage_dir.clone()
        } else {
            self.stores_data_root.join(store_id)
        }
    }

    /// Seed a `ResourceRecord` (with id == name == `resource_id`) and a
    /// `WrapperResourcePairRecord` directly into the "test" store.
    /// Convenience wrapper around [`Self::ensure_pair_in_store`].
    pub fn ensure_pair(&self, wrapper: &str, resource_id: &str) -> Result<()> {
        self.ensure_pair_in_store("test", wrapper, resource_id)
    }

    /// Same as [`Self::ensure_pair`] but targets a specific store
    /// directory. Use for tests that remap the daemon to a
    /// non-default store. Idempotent. It bypasses the admin API's generated
    /// `res_<hex>` id so tests can choose deterministic identifiers.
    pub fn ensure_pair_in_store(
        &self,
        store_id: &str,
        wrapper: &str,
        resource_id: &str,
    ) -> Result<()> {
        use underwrap_backend_store::StorageBackend;
        use underwrap_protocol::entities::{
            EntityCreator, PAIRS_DOCUMENT_SECRET_ID, PairsDocument, RESOURCES_DOCUMENT_SECRET_ID,
            ResourceRecord, ResourcesDocument, WrapperResourcePairRecord,
        };

        let dir = self.store_dir_for(store_id);
        let store = underwrap_backend_store::PlainBackend::open(&dir)?;
        let mut resources: ResourcesDocument = match store
            .read(RESOURCES_DOCUMENT_SECRET_ID)
            .context("read resources")?
        {
            Some(m) => serde_json::from_slice::<ResourcesDocument>(m.as_ref())
                .context("decode resources")?,
            None => ResourcesDocument::default(),
        };
        if resources.find_by_id(resource_id).is_none() {
            resources
                .insert(ResourceRecord {
                    id: resource_id.to_string(),
                    name: resource_id.to_string(),
                    created_by: EntityCreator::Operator {
                        id: "test-op".into(),
                        username: "tester".into(),
                    },
                    created_at: 1,
                })
                .map_err(|e| anyhow::anyhow!("insert resource: {e}"))?;
            store.write(
                RESOURCES_DOCUMENT_SECRET_ID,
                &serde_json::to_vec(&resources).context("encode resources")?,
            )?;
        }
        let mut pairs: PairsDocument =
            match store.read(PAIRS_DOCUMENT_SECRET_ID).context("read pairs")? {
                Some(m) => {
                    serde_json::from_slice::<PairsDocument>(m.as_ref()).context("decode pairs")?
                }
                None => PairsDocument::default(),
            };
        if pairs.find(wrapper, resource_id).is_none() {
            pairs
                .insert(WrapperResourcePairRecord {
                    wrapper: wrapper.to_string(),
                    resource_id: resource_id.to_string(),
                    created_by: EntityCreator::Operator {
                        id: "test-op".into(),
                        username: "tester".into(),
                    },
                    created_at: 1,
                    last_used_at: None,
                })
                .map_err(|e| anyhow::anyhow!("insert pair: {e}"))?;
            store.write(
                PAIRS_DOCUMENT_SECRET_ID,
                &serde_json::to_vec(&pairs).context("encode pairs")?,
            )?;
        }
        Ok(())
    }

    /// Write a secret through the admin API *without* touching the
    /// policy document first. Used by tests that exercise the admin
    /// write path in isolation.
    pub async fn admin_write_raw(
        &self,
        store: &str,
        _wrapper: &str,
        resource_id: &str,
        material: &str,
    ) -> Result<()> {
        let path = format!(
            "/v1/admin/stores/{}/secrets/{}/env/{}",
            pe(store),
            pe(resource_id),
            pe(FIXTURE_ENV_KEY),
        );
        let response = self
            .admin_request(reqwest::Method::PUT, &path)
            .json(&underwrap_protocol::service::AdminSecretWriteRequest {
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

    /// Probe whether this daemon's runtime credential is currently
    /// authorized at the backend. Issues an empty `POST /v1/wrappers`
    /// register call — which round-trips without changing any state
    /// — and reports success/failure. Used by tests that revoke the
    /// credential mid-flight and want to assert the next runtime
    /// connection fails.
    pub async fn direct_service_status(&self) -> Result<()> {
        let client = self.direct_service_client()?;
        let response = self.empty_runtime_registration(client).await?;
        anyhow::ensure!(
            response.status().is_success(),
            "service API returned {}",
            response.status()
        );
        Ok(())
    }

    /// Issue the same state-free runtime registration using one of the two
    /// synthetic fixture identities. This is intentionally narrower than a
    /// general credential-building API so certificate-rotation tests cannot
    /// accidentally send secret-bearing runtime requests. The response remains
    /// available so tests can distinguish an HTTP rejection from construction
    /// or transport failures.
    pub(crate) async fn direct_service_status_as(
        &self,
        identity: TestRuntimeIdentity,
    ) -> Result<reqwest::Response> {
        let (cert_pem, key_pem) = match identity {
            TestRuntimeIdentity::Local => (TEST_CLIENT_LOCAL_PEM, TEST_CLIENT_LOCAL_KEY),
            TestRuntimeIdentity::Tunnel => (TEST_CLIENT_TUNNEL_PEM, TEST_CLIENT_TUNNEL_KEY),
        };
        let client =
            self.direct_service_client_from_pem(cert_pem.as_bytes(), key_pem.as_bytes())?;
        self.empty_runtime_registration(client).await
    }

    async fn empty_runtime_registration(
        &self,
        client: reqwest::Client,
    ) -> Result<reqwest::Response> {
        Ok(client
            .post(format!("{}/v1/wrappers", self.service_url))
            .json(&underwrap_protocol::service::RuntimeWrapperRegisterRequest { wrappers: vec![] })
            .send()
            .await?)
    }

    /// Build an mTLS-authenticated reqwest client that authenticates as
    /// this daemon's runtime client instance. Tests use it to bypass
    /// the daemon entirely and call backend `/v1/*` endpoints directly,
    /// e.g. to prove the backend gates writes server-side regardless
    /// of what the client tries to send.
    pub fn direct_service_client(&self) -> Result<reqwest::Client> {
        let cert_pem = std::fs::read(&self.service_client_cert)?;
        let key_pem = std::fs::read(&self.service_client_key)?;
        self.direct_service_client_from_pem(&cert_pem, &key_pem)
    }

    fn direct_service_client_from_pem(
        &self,
        cert_pem: &[u8],
        key_pem: &[u8],
    ) -> Result<reqwest::Client> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let ca_pem = std::fs::read(&self.service_ca_cert)?;
        let mut identity_pem = cert_pem.to_vec();
        identity_pem.extend_from_slice(key_pem);
        Ok(reqwest::Client::builder()
            .use_rustls_tls()
            .add_root_certificate(reqwest::Certificate::from_pem(&ca_pem)?)
            .identity(reqwest::Identity::from_pem(&identity_pem)?)
            .build()?)
    }

    pub fn service_url(&self) -> &str {
        &self.service_url
    }

    pub fn store_data_dir(&self, store_id: &str) -> PathBuf {
        self.stores_data_root.join(store_id)
    }

    pub async fn rotate_store_discard_response(&self, store_id: &str) -> Result<()> {
        let response = self
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/{store_id}/rotate-key"),
            )
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        drop(response);
        Ok(())
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

    pub async fn create_alternate_client_instance(&self, store_id: &str) -> Result<String> {
        let response = self
            .admin_request(reqwest::Method::POST, "/v1/admin/client-instances")
            .json(
                &underwrap_protocol::service::AdminClientInstanceCreateRequest {
                    display_name: "alternate runtime client".to_string(),
                    current_store_id: store_id.to_string(),
                    status: underwrap_protocol::service::AdminClientInstanceStatus::Active,
                    cert_pem: pki::TEST_CLIENT_TUNNEL_PEM.to_string(),
                    auto_create_wrapper_assets: None,
                },
            )
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        let item: underwrap_protocol::service::AdminClientInstanceListItem =
            response.json().await?;
        Ok(item.id)
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
    ) -> Result<underwrap_protocol::service::AdminClientInstanceListItem> {
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
            .admin_request(
                reqwest::Method::GET,
                &format!("/v1/admin/stores/{}/resources", pe(store)),
            )
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
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/{}/resources", pe(store)),
            )
            .json(&underwrap_protocol::service::AdminResourceCreateRequest {
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
        id: &str,
    ) -> Result<underwrap_protocol::service::AdminResourceDeleteResponse> {
        let response = self
            .admin_request(
                reqwest::Method::DELETE,
                &format!("/v1/admin/stores/{}/resources/{}", pe(store), pe(id)),
            )
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

    /// Load a resource's canonical blob directly from the on-disk store. The
    /// admin API exposes per-entry operations rather than a raw-blob read, so
    /// tests inspect canonical bytes through the storage backend.
    pub fn get_resource_blob(
        &self,
        store: &str,
        id: &str,
    ) -> Result<underwrap_protocol::resource_blob::ResourceBlob> {
        let dir = self.store_dir_for(store);
        let backend = underwrap_backend_store::PlainBackend::open(&dir)?;
        underwrap_backend_store::load_resource_blob(&backend, id)
    }

    /// Persist a resource blob directly to the on-disk store. See
    /// [`Self::get_resource_blob`] for the rationale.
    pub fn put_resource_blob(
        &self,
        store: &str,
        id: &str,
        blob: &underwrap_protocol::resource_blob::ResourceBlob,
    ) -> Result<()> {
        let dir = self.store_dir_for(store);
        let backend = underwrap_backend_store::PlainBackend::open(&dir)?;
        underwrap_backend_store::save_resource_blob(&backend, id, blob)?;
        Ok(())
    }

    /// Directly inject bytes to exercise corrupt-storage fail-closed paths.
    pub fn put_raw_resource_bytes(&self, store: &str, id: &str, bytes: &[u8]) -> Result<()> {
        let dir = self.store_dir_for(store);
        let backend = underwrap_backend_store::PlainBackend::open(&dir)?;
        underwrap_backend_store::StorageBackend::write(
            &backend,
            &format!("resources/{id}"),
            bytes,
        )?;
        Ok(())
    }

    pub async fn delete_pair(
        &self,
        store: &str,
        wrapper: &str,
        resource_id: &str,
    ) -> Result<underwrap_protocol::service::AdminPairDeleteResponse> {
        let response = self
            .admin_request(
                reqwest::Method::DELETE,
                &format!(
                    "/v1/admin/stores/{}/pairs/{}/{}",
                    pe(store),
                    pe(wrapper),
                    pe(resource_id),
                ),
            )
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

    pub async fn create_pair(
        &self,
        store: &str,
        wrapper: &str,
        resource_id: &str,
    ) -> Result<underwrap_protocol::service::AdminPairCreateResponse> {
        let response = self
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/{}/pairs", pe(store)),
            )
            .json(&underwrap_protocol::service::AdminPairCreateRequest {
                wrapper: wrapper.to_string(),
                resource_id: resource_id.to_string(),
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
            .admin_request(
                reqwest::Method::GET,
                &format!("/v1/admin/stores/{}/pairs", pe(store)),
            )
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        Ok(response.json().await?)
    }

    pub async fn get_policy(&self, store: &str) -> Result<PolicyDocument> {
        let permissions = self.list_permissions(store).await?;
        Ok(PolicyDocument { permissions })
    }

    pub async fn put_policy(&self, store: &str, doc: &PolicyDocument) -> Result<()> {
        // The backend requires a (wrapper, resource) pair record for
        // every permission tuple; seed any missing ones before the
        // policy lands. Idempotent.
        for p in &doc.permissions {
            self.ensure_pair(&p.wrapper, &p.resource_id)?;
        }
        // Wholesale replace semantics: drop any permission currently in
        // the store that isn't in `doc`, then upsert every row in
        // `doc`. Test harness only — the admin SPA edits permissions
        // one at a time via the granular endpoints.
        let existing = self.list_permissions(store).await?;
        for permission in &existing {
            let still_present = doc.permissions.iter().any(|p| {
                p.client_instance == permission.client_instance
                    && p.wrapper == permission.wrapper
                    && p.resource_id == permission.resource_id
            });
            if !still_present {
                self.admin_delete_permission(
                    store,
                    &permission.client_instance,
                    &permission.wrapper,
                    &permission.resource_id,
                )
                .await?;
            }
        }
        for permission in &doc.permissions {
            self.admin_upsert_permission(store, permission).await?;
        }
        Ok(())
    }

    async fn list_permissions(&self, store: &str) -> Result<Vec<WrapperPermission>> {
        let response = self
            .admin_request(
                reqwest::Method::GET,
                &format!("/v1/admin/stores/{}/wrapper-permissions", pe(store)),
            )
            .send()
            .await?;
        anyhow::ensure!(
            response.status().is_success(),
            "admin API returned {}",
            response.status()
        );
        let body: underwrap_protocol::service::AdminWrapperPermissionsListResponse =
            response.json().await?;
        Ok(body.permissions)
    }

    async fn admin_delete_permission(
        &self,
        store: &str,
        client_instance: &str,
        wrapper: &str,
        resource_id: &str,
    ) -> Result<()> {
        let response = self
            .admin_request(
                reqwest::Method::DELETE,
                &format!(
                    "/v1/admin/stores/{}/wrapper-permissions/{}/{}/{}",
                    pe(store),
                    pe(client_instance),
                    pe(wrapper),
                    pe(resource_id),
                ),
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

    pub async fn upsert_permission(
        &self,
        store: &str,
        permission: &WrapperPermission,
    ) -> Result<()> {
        self.ensure_pair(&permission.wrapper, &permission.resource_id)?;
        self.admin_upsert_permission(store, permission).await
    }

    pub async fn delete_permission(
        &self,
        store: &str,
        client_instance: &str,
        wrapper: &str,
        resource_id: &str,
    ) -> Result<()> {
        self.admin_delete_permission(store, client_instance, wrapper, resource_id)
            .await
    }

    /// Convenience: build an `Unlocked` wrapper permission for the
    /// given wrapper + resource_id on this harness's mTLS-bound client
    /// instance. Used by almost every read-flow test.
    pub fn current_permission(&self, wrapper: &str, resource_id: &str) -> WrapperPermission {
        WrapperPermission {
            client_instance: self.client_instance.clone(),
            wrapper: wrapper.to_string(),
            resource_id: resource_id.to_string(),
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
        self.backend_shutdown.notify_one();

        let backend_join = tokio::task::spawn_blocking(move || self.backend_handle.join());
        let (daemon_result, backend_result) = tokio::join!(self.handle, backend_join);

        let daemon_result = match daemon_result {
            Ok(result) => result.context("daemon exited with an error"),
            Err(error) => Err(error).context("joining daemon task"),
        };
        let backend_result = match backend_result {
            Ok(Ok(result)) => result.context("backend exited with an error"),
            Ok(Err(_)) => anyhow::bail!("backend thread panicked during shutdown"),
            Err(error) => Err(error).context("joining backend shutdown task"),
        };

        match (daemon_result, backend_result) {
            (Err(daemon_error), Err(backend_error)) => anyhow::bail!(
                "daemon and backend shutdown failed: daemon: {daemon_error:#}; backend: {backend_error:#}"
            ),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }
}
