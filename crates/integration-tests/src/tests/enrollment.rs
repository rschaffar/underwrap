use super::*;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use tokio::net::UnixStream;
use tokio::sync::Notify;
use underwrap_protocol::config::{BackendServiceConfig, DaemonConfig, StorageConfig, StoreRecord};
use underwrap_protocol::ipc::{read_frame_async, write_frame_async};
use underwrap_protocol::message::{ErrorCode, WrapperRequest, WrapperResponse};
use zeroize::Zeroizing;

struct EnrollmentBackend {
    admin_url: String,
    service_url: String,
    service_ca_cert: PathBuf,
    server_cert_fingerprint: String,
    admin_session_cookie: String,
    admin_csrf_token: String,
    admin_client: reqwest::Client,
    shutdown: Arc<Notify>,
    handle: thread::JoinHandle<Result<()>>,
    _tempdir: tempfile::TempDir,
}

impl EnrollmentBackend {
    async fn start() -> Result<Self> {
        let tempdir = tempfile::TempDir::new()?;
        let base = tempdir.path();

        let https_port = pick_unused_port()?;
        let admin_url = format!("https://127.0.0.1:{https_port}");
        let service_url = admin_url.clone();

        let pki = generate_backend_pki(base)?;
        let session_key_file = base.join("admin-session-key");
        std::fs::write(
            &session_key_file,
            [9u8; underwrap_backendd::auth::SESSION_KEY_LEN],
        )?;

        let stores_file = base.join("stores.json");
        std::fs::write(
            &stores_file,
            serde_json::to_vec_pretty(&underwrap_protocol::config::StoreRegistryDocument {
                version: 1,
                stores: vec![StoreRecord {
                    id: "test".into(),
                    name: "test".into(),
                    description: Some("enrollment test store".into()),
                    created_at: Some(100),
                    storage: StorageConfig::Plain {
                        dir: base.join("store-data"),
                    },
                }],
            })?,
        )?;

        let config = underwrap_backendd::server::BackendConfig {
            stores_file: Some(stores_file),
            client_instances_file: Some(base.join("client-instances.json")),
            operators_file: Some(base.join("operators.json")),
            enrollment: Some(underwrap_backendd::server::EnrollmentConfig {
                invites_file: Some(base.join("enrollment-invites.json")),
                default_invite_ttl_secs: Some(3600),
            }),
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
                stores_data_roots: Some(vec![base.join("stores-data-root")]),
                keys_dir: Some(base.join("keys")),
            }),
        };

        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = Arc::clone(&shutdown);
        let handle = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                underwrap_backendd::server::run(config, Some(shutdown_clone)).await
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

        let server_cert_fingerprint = compute_cert_fingerprint(&pki.server_cert)?;

        Ok(Self {
            admin_url,
            service_url,
            service_ca_cert: pki.ca_cert,
            server_cert_fingerprint,
            admin_session_cookie,
            admin_csrf_token,
            admin_client,
            shutdown,
            handle,
            _tempdir: tempdir,
        })
    }

    async fn create_invite(
        &self,
        display_name: &str,
        host: &str,
        instance: &str,
    ) -> Result<underwrap_protocol::service::AdminEnrollmentInviteCreateResponse> {
        let response = self
            .admin_client
            .post(format!("{}/v1/admin/enrollment-invites", self.admin_url))
            .header(
                reqwest::header::COOKIE,
                csrf_cookie_header(&self.admin_session_cookie, &self.admin_csrf_token),
            )
            .header("X-CSRF-Token", &self.admin_csrf_token)
            .json(
                &underwrap_protocol::service::AdminEnrollmentInviteCreateRequest {
                    display_name: display_name.to_string(),
                    hostname_hint: Some(host.to_string()),
                    instance_name: instance.to_string(),
                    current_store_id: "test".to_string(),
                    expires_in_secs: None,
                },
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

    async fn stop(self) -> Result<()> {
        self.shutdown.notify_one();
        self.handle
            .join()
            .map_err(|_| anyhow::anyhow!("backend thread panicked"))??;
        Ok(())
    }
}

struct GeneratedBackendPki {
    ca_cert: PathBuf,
    server_cert: PathBuf,
    server_key: PathBuf,
}

fn generate_backend_pki(base: &Path) -> Result<GeneratedBackendPki> {
    let pki_dir = base.join("pki");
    std::fs::create_dir_all(&pki_dir)?;

    // Server CA only — the backend's TLS cert is signed by this CA so the
    // daemon's reqwest client can verify it. Client certs are now
    // fingerprint-pinned and need no CA at all.
    let ca_key = rcgen::KeyPair::generate().context("generating server CA key")?;
    let mut ca_dn = rcgen::DistinguishedName::new();
    ca_dn.push(rcgen::DnType::CommonName, "underwrap-server-test-ca");
    let mut ca_params = rcgen::CertificateParams::default();
    ca_params.distinguished_name = ca_dn;
    ca_params.serial_number = Some(rcgen::SerialNumber::from(1u64));
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        rcgen::KeyUsagePurpose::DigitalSignature,
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];
    let ca = rcgen::CertifiedIssuer::self_signed(ca_params, ca_key)
        .context("issuing server CA certificate")?;

    let server_key = rcgen::KeyPair::generate().context("generating backend TLS key")?;
    let mut server_dn = rcgen::DistinguishedName::new();
    server_dn.push(rcgen::DnType::CommonName, "localhost");
    let mut server_params =
        rcgen::CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()])
            .context("building backend TLS certificate params")?;
    server_params.distinguished_name = server_dn;
    server_params.serial_number = Some(rcgen::SerialNumber::from(2u64));
    server_params.key_usages = vec![
        rcgen::KeyUsagePurpose::DigitalSignature,
        rcgen::KeyUsagePurpose::KeyEncipherment,
    ];
    server_params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    server_params.use_authority_key_identifier_extension = true;
    let server_cert = server_params
        .signed_by(&server_key, &ca)
        .context("issuing backend TLS certificate")?;

    let ca_cert_path = pki_dir.join("ca.pem");
    let server_cert_path = pki_dir.join("server.pem");
    let server_key_path = pki_dir.join("server.key");
    std::fs::write(&ca_cert_path, ca.pem())?;
    std::fs::write(&server_cert_path, server_cert.pem())?;
    std::fs::write(&server_key_path, server_key.serialize_pem())?;

    Ok(GeneratedBackendPki {
        ca_cert: ca_cert_path,
        server_cert: server_cert_path,
        server_key: server_key_path,
    })
}

fn compute_cert_fingerprint(cert_pem_path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let pem = std::fs::read(cert_pem_path)?;
    let mut reader = std::io::BufReader::new(pem.as_slice());
    let cert = rustls_pemfile::certs(&mut reader)
        .next()
        .context("server cert PEM contained no certificate")??;
    let digest = Sha256::digest(cert.as_ref());
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

async fn wait_for_path(path: &Path) -> Result<()> {
    for _ in 0..3000 {
        if path.exists() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    anyhow::bail!("path never appeared: {}", path.display())
}

#[tokio::test]
async fn unenrolled_wrapper_read_returns_enrollment_guidance() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let wrapper_socket = tempdir.path().join("wrapper-gh.sock");
    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = Arc::clone(&shutdown);
    let ca_file = tempdir.path().join("ca.pem");
    std::fs::write(&ca_file, crate::harness::TEST_CA_PEM).unwrap();
    let config = DaemonConfig {
        backend: BackendServiceConfig {
            url: "https://backend.example.internal".into(),
            ca_file: Some(ca_file),
            client_cert_file: None,
            client_key_file: None,
        },
        catalog_host: "deepthought".into(),
        catalog_instance: "work".into(),
        allowed_users: Vec::new(),
        wrappers: HashMap::from([(
            "gh-authorized".to_string(),
            wrapper("/usr/bin/gh", "github/token", "GH_TOKEN"),
        )]),
    };
    let mut config = config;
    config.wrappers.get_mut("gh-authorized").unwrap().socket = wrapper_socket.clone();

    let handle =
        tokio::spawn(async move { underwrapd::server::run(config, Some(shutdown_clone)).await });

    wait_for_path(&wrapper_socket).await.unwrap();
    let mut stream = UnixStream::connect(&wrapper_socket).await.unwrap();
    write_frame_async(&mut stream, &WrapperRequest::Read { scope: None })
        .await
        .unwrap();
    let response: WrapperResponse = read_frame_async(&mut stream).await.unwrap();
    match response {
        WrapperResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::Unauthorized));
            assert!(message.contains("not enrolled yet"));
            assert!(message.contains("/enroll?host=deepthought&instance=work"));
        }
        other => panic!("expected unenrolled guidance, got {other:?}"),
    }

    shutdown.notify_one();
    handle.await.unwrap().unwrap();
}

fn mtls_client(ca_pem: &[u8], cert_pem: &[u8], key_pem: &[u8]) -> reqwest::Client {
    let mut identity_pem = cert_pem.to_vec();
    identity_pem.extend_from_slice(key_pem);
    reqwest::Client::builder()
        .use_rustls_tls()
        .add_root_certificate(reqwest::Certificate::from_pem(ca_pem).unwrap())
        .identity(reqwest::Identity::from_pem(&identity_pem).unwrap())
        .build()
        .unwrap()
}

#[tokio::test]
async fn invite_first_enrollment_flow_installs_credential_and_enables_runtime_access() {
    let backend = EnrollmentBackend::start().await.unwrap();
    let invite = backend
        .create_invite("deepthought work", "deepthought", "work")
        .await
        .unwrap();

    let install_dir = tempfile::TempDir::new().unwrap();
    let client_cert = install_dir.path().join("client-work.pem");
    let client_key = install_dir.path().join("client-work.key");
    let enroll_cli = frontend_enroll::Cli {
        url: Some(backend.admin_url.clone()),
        invite_id: invite.invite.id.clone(),
        accept_server_ca_fingerprint: Some(backend.server_cert_fingerprint.clone()),
        client_cert_out: Some(client_cert.clone()),
        client_key_out: Some(client_key.clone()),
        instance_config: None,
    };
    let enroll_secret = Zeroizing::new(invite.enrollment_secret.clone());
    tokio::task::spawn_blocking(move || {
        frontend_enroll::run_with_secret(enroll_cli, enroll_secret)
    })
    .await
    .unwrap()
    .unwrap();

    let cert_pem = std::fs::read(&client_cert).unwrap();
    let key_pem = std::fs::read(&client_key).unwrap();
    let ca_pem = std::fs::read(&backend.service_ca_cert).unwrap();
    assert!(String::from_utf8_lossy(&cert_pem).contains("BEGIN CERTIFICATE"));
    assert!(String::from_utf8_lossy(&key_pem).contains("BEGIN PRIVATE KEY"));

    let client = mtls_client(&ca_pem, &cert_pem, &key_pem);
    let status = client
        .get(format!("{}/v1/status", backend.service_url))
        .send()
        .await
        .unwrap();
    assert!(
        status.status().is_success(),
        "status endpoint returned {}",
        status.status()
    );
    let body: underwrap_protocol::service::ServiceStatusResponse = status.json().await.unwrap();
    assert!(body.ok);

    let instances = backend
        .admin_client
        .get(format!("{}/v1/admin/client-instances", backend.admin_url))
        .header(
            reqwest::header::COOKIE,
            csrf_cookie_header(&backend.admin_session_cookie, &backend.admin_csrf_token),
        )
        .send()
        .await
        .unwrap();
    assert!(instances.status().is_success());
    let instances: underwrap_protocol::service::AdminClientInstancesListResponse =
        instances.json().await.unwrap();
    assert_eq!(instances.items.len(), 1);
    assert_eq!(body.principal, instances.items[0].id);
    assert_eq!(instances.items[0].display_name, "deepthought work");
    assert_eq!(instances.items[0].instance_name, "work");
    assert_eq!(
        instances.items[0].hostname_hint.as_deref(),
        Some("deepthought")
    );

    backend.stop().await.unwrap();
}

#[tokio::test]
async fn re_enrollment_replaces_old_credential_end_to_end() {
    let backend = EnrollmentBackend::start().await.unwrap();
    let first_invite = backend
        .create_invite("deepthought work", "deepthought", "work")
        .await
        .unwrap();

    let install_dir = tempfile::TempDir::new().unwrap();
    let first_cert = install_dir.path().join("client-work-first.pem");
    let first_key = install_dir.path().join("client-work-first.key");
    let first_cli = frontend_enroll::Cli {
        url: Some(backend.admin_url.clone()),
        invite_id: first_invite.invite.id.clone(),
        accept_server_ca_fingerprint: Some(backend.server_cert_fingerprint.clone()),
        client_cert_out: Some(first_cert.clone()),
        client_key_out: Some(first_key.clone()),
        instance_config: None,
    };
    tokio::task::spawn_blocking(move || {
        frontend_enroll::run_with_secret(first_cli, Zeroizing::new(first_invite.enrollment_secret))
    })
    .await
    .unwrap()
    .unwrap();

    let second_invite = backend
        .create_invite("deepthought work", "deepthought", "work")
        .await
        .unwrap();
    let second_cert = install_dir.path().join("client-work-second.pem");
    let second_key = install_dir.path().join("client-work-second.key");
    let second_cli = frontend_enroll::Cli {
        url: Some(backend.admin_url.clone()),
        invite_id: second_invite.invite.id.clone(),
        accept_server_ca_fingerprint: Some(backend.server_cert_fingerprint.clone()),
        client_cert_out: Some(second_cert.clone()),
        client_key_out: Some(second_key.clone()),
        instance_config: None,
    };
    tokio::task::spawn_blocking(move || {
        frontend_enroll::run_with_secret(
            second_cli,
            Zeroizing::new(second_invite.enrollment_secret),
        )
    })
    .await
    .unwrap()
    .unwrap();

    let ca_pem = std::fs::read(&backend.service_ca_cert).unwrap();
    let first_client = mtls_client(
        &ca_pem,
        &std::fs::read(&first_cert).unwrap(),
        &std::fs::read(&first_key).unwrap(),
    );
    let first_status = first_client
        .get(format!("{}/v1/status", backend.service_url))
        .send()
        .await
        .unwrap();
    assert_eq!(first_status.status(), reqwest::StatusCode::UNAUTHORIZED);

    let second_client = mtls_client(
        &ca_pem,
        &std::fs::read(&second_cert).unwrap(),
        &std::fs::read(&second_key).unwrap(),
    );
    let second_status = second_client
        .get(format!("{}/v1/status", backend.service_url))
        .send()
        .await
        .unwrap();
    assert!(second_status.status().is_success());

    let instances = backend
        .admin_client
        .get(format!("{}/v1/admin/client-instances", backend.admin_url))
        .header(
            reqwest::header::COOKIE,
            csrf_cookie_header(&backend.admin_session_cookie, &backend.admin_csrf_token),
        )
        .send()
        .await
        .unwrap();
    assert!(instances.status().is_success());
    let instances: underwrap_protocol::service::AdminClientInstancesListResponse =
        instances.json().await.unwrap();
    assert_eq!(instances.items.len(), 1);

    backend.stop().await.unwrap();
}
