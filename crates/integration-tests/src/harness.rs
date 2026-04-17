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

/// A running test daemon with temp dirs and sockets.
#[allow(dead_code)]
pub struct TestDaemon {
    pub wrapper_sockets: HashMap<String, PathBuf>,
    pub admin_url: String,
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
        wrappers: HashMap<String, WrapperDef>,
        credentials: HashMap<String, CredentialDef>,
    ) -> Result<Self> {
        Self::start_service_mode(wrappers, credentials, "local", "underwrap-local").await
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
        let backend_config = underwrap_backendd::server::BackendConfig {
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
            admin: Some(underwrap_backendd::server::AdminConfig {
                listen: format!("127.0.0.1:{admin_port}"),
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

        wait_for_admin(&admin_url).await?;
        wait_for_service(
            &service_url,
            &ready_pki.ca_cert,
            &ready_pki.client_cert,
            &ready_pki.client_key,
        )
        .await?;

        let config = DaemonConfig {
            sockets: SocketConfig::default(),
            backend: BackendSocketConfig {
                mode: underwrap_protocol::config::BackendMode::Service,
                socket: "/tmp/unused-backend.sock".into(),
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

        for socket in wrapper_sockets.values() {
            wait_for_path(socket).await?;
        }

        Ok(Self {
            wrapper_sockets,
            admin_url,
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

    /// Write a secret through the backend admin API.
    pub async fn store_secret(&self, store: &str, secret_id: &str, material: &str) -> Result<()> {
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/v1/admin/secret/write", self.admin_url))
            .json(&underwrap_protocol::service::AdminSecretWriteRequest {
                store: store.to_string(),
                secret_id: secret_id.to_string(),
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

    pub async fn get_policy(&self, store: &str) -> Result<Option<PolicyDocument>> {
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/v1/admin/policy/get", self.admin_url))
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
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/v1/admin/policy/put", self.admin_url))
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
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/v1/admin/grant/upsert", self.admin_url))
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
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/v1/admin/grant/delete", self.admin_url))
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

pub(crate) const TEST_CA_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDGTCCAgGgAwIBAgIUDzLEbd9HNdAV+jW3B9DEKAVIjJcwDQYJKoZIhvcNAQEL
BQAwHDEaMBgGA1UEAwwRdW5kZXJ3cmFwLXRlc3QtY2EwHhcNMjYwNDE2MDAzNzAx
WhcNMzYwNDEzMDAzNzAxWjAcMRowGAYDVQQDDBF1bmRlcndyYXAtdGVzdC1jYTCC
ASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoCggEBAOEpAbI8m/9ph1hN2ESZd5Kv
N6whju6KszjKCnWF1/HjbzOWgGHAGaLELstS5d+1bMlA0EXTlmp4I5uN/x2Fm5EI
GXLGopnadGhnlfGQ4z9Szpfr6RxHRn+MxvWdLavrAStDuUbRwxqJmIqcHmL4WJTS
c69S9s/1JFgWYVm3nQAjLj0EH2al7HJ6Jygu9Q2eZl9ueIjBGsT36xY44odYMqPF
64W3wHgYVBXZK0+HkxcocHa7ciQ1Qgmyc0krTNknjR5zep/yq7ReIB+CqucCsr4e
92eE57Wf3UCXu49liVujTPC56ivMMsapwbjqiyjiYHKGf6VlRsd990VFr+Ar3ZMC
AwEAAaNTMFEwHQYDVR0OBBYEFJUnI6Bxe5iBsEMutUi2/YrD9XXeMB8GA1UdIwQY
MBaAFJUnI6Bxe5iBsEMutUi2/YrD9XXeMA8GA1UdEwEB/wQFMAMBAf8wDQYJKoZI
hvcNAQELBQADggEBANbo9Yc5jEKsvlLAErGbYrWTq72V4KzuIIjhIbFGbtITkiB2
oa9e1JCh7HYXIgVZNyXkP817fdPt8uHUAmhgwdOtSNwIu+nUI7L3GCcb6gvL4cRz
AZU4o2LbjuUmfRl7BTK7sL/jKco9EbqYQqnxRvxnlvHrzlbM+gnTtBnNNy0sVzP4
orP4J8SCiNNioevdLTsj8EUwTxF3NNuDcZPyPLKthWyuHmlKinyPihHOxT5QXDN+
dlg0gy8NbKgHU/E+k3R5jMBJaeNfHCaFF8otAtpqoheBX3u5b1MwTQ7EVLsfZN1V
+BGi1gTGHPsIv0CDVx/LkVmaqwv+rhPZqv4Ymhg=
-----END CERTIFICATE-----
"#;

pub(crate) const TEST_SERVER_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDQzCCAiugAwIBAgIUVnjkCKXzQiyHyEsQ3ey4lgWkrS8wDQYJKoZIhvcNAQEL
BQAwHDEaMBgGA1UEAwwRdW5kZXJ3cmFwLXRlc3QtY2EwHhcNMjYwNDE2MDAzNzAy
WhcNMzYwNDEzMDAzNzAyWjAUMRIwEAYDVQQDDAlsb2NhbGhvc3QwggEiMA0GCSqG
SIb3DQEBAQUAA4IBDwAwggEKAoIBAQCm7XnsD0fkZ9rg6rK5De5RWuP8LLAIhbri
QPgRhin2pkjv6rYrwAZ2yuTtG+Agx98FIFI4jczJVbsHPEAJ8L0JlWy0GnFQqYu2
wLZWMhDlCOALKdJH72H9zyg/32KMWI+WYqZr90WZ7hTzlNdA0C5vH9mOHOAKIrCF
F1z2MGmTkR5h+26HCzoCRtFZdaKSjWsTF7GJEvK+0zpBh2lelP6cnj5usuleXoXm
+QYRoz8Gwh5Cwtq0f7nrHvrG0SakrGqqCAdYUIeSJje12PvsRWaLs+1gS3MP8eEy
8vy2a6a4yM7rQLhBItk37PK/3RdJMeAipDUf1PghzH5ZR+sEhTz7AgMBAAGjgYQw
gYEwGgYDVR0RBBMwEYIJbG9jYWxob3N0hwR/AAABMBMGA1UdJQQMMAoGCCsGAQUF
BwMBMA4GA1UdDwEB/wQEAwIFoDAdBgNVHQ4EFgQUYopvBM3Lphog2L94aAvR/tL7
hq0wHwYDVR0jBBgwFoAUlScjoHF7mIGwQy61SLb9isP1dd4wDQYJKoZIhvcNAQEL
BQADggEBAIjG16boNh4U5lXJPMO4fa3+3bAao1CPdZJjqTw1aawQg7Cp7vQSULvg
BAz2xmGjdYayQALCA/9AyWCKFdGQ3pLDT2Vt2ecU7sXoFz8XnnX0mUD9VgqAyifi
eAo4A7bTjlA16n9RCtDm7wYqXnL4mjoy8zisfiMJ3MGxltwiT4LrTSwYOwVxjbOR
weP4Wyayn/7F7PhUY4i2/8ykNw77z7BOLsiEkFfMGsySTkdBgxQPIrj59IadTh7I
w/42D54cE+Qq65DQvBADKmLYHj50M9q7dJvBh4vNEQYNPEGVlYdG02EEGyYMmggo
Ec7n9CeusZNwlgN7yGlJ7ZGZA8ZPT1g=
-----END CERTIFICATE-----
"#;

pub(crate) const TEST_SERVER_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQCm7XnsD0fkZ9rg
6rK5De5RWuP8LLAIhbriQPgRhin2pkjv6rYrwAZ2yuTtG+Agx98FIFI4jczJVbsH
PEAJ8L0JlWy0GnFQqYu2wLZWMhDlCOALKdJH72H9zyg/32KMWI+WYqZr90WZ7hTz
lNdA0C5vH9mOHOAKIrCFF1z2MGmTkR5h+26HCzoCRtFZdaKSjWsTF7GJEvK+0zpB
h2lelP6cnj5usuleXoXm+QYRoz8Gwh5Cwtq0f7nrHvrG0SakrGqqCAdYUIeSJje1
2PvsRWaLs+1gS3MP8eEy8vy2a6a4yM7rQLhBItk37PK/3RdJMeAipDUf1PghzH5Z
R+sEhTz7AgMBAAECggEAAOSnEQWek4ZKvDFaoMrbn5Z7lHnYvJ2akQ4FRcgQqWqH
sWDxde0icQ2c3qKWNxawBs2peEv/RdnY6lephWQYXtjB1Xo0E65p5PcbAqhklr/O
vcCM3ihVvznrM+GfcV4im+IN6eikzVy3bVDlO0o3NWOs56nCjnX+Y81AM2uxj5c7
pHxBY9fyby6bug/uD0WkptEDBXaoSH/hk8r3VDfZ5ZFjDiCbfCwvoN5mvR7pcWeA
F6TchuIyFOUMXjqOihmT4dib3zVFeqpktcsNZAjhBcgmbVU7JctQEoEfzQpF4MyO
PTJVriKv467HwmzLx+KGuNZrUDCLLkVlWAuCbw+DwQKBgQDavLiO0gfE1siw4sAG
M6HO/52NQdwDNCqM9IY5kPsxVUKPQKO25+iGiPyoNPUTtZLxCK0cKpRZGcGb7dJt
vfDIQKzjVKhg6tSi7rv72uVO0oTO3rIayroDKdl0bEyMWKG4GskBHCD/ILoeuxlp
XR93bL0VW/fLzCjBjvtugoDHMwKBgQDDXU4oo31qIAY5SDzSfU/siArOwpUNW9EE
K4NJtOQujfb4ic3G0jL6tBvNLT1U8RqdymxAL1qc27vXGgXy0y4FJNxR5Z7jiTyf
WUtYVsZ/4TXHxAnoEceJ+Kbv1b+HGUQE/NMPYRbC8jUuowU7igXMdcy7jHxLX+0F
ty5y2DMTGQKBgC2kUXJjuwFEvA3E0sK4hVv3FH4ELw9wFCzjXRqn+PI7Uy8nEsI4
s9lWvjmvCzB4o9etssl8zQWmjTKnNlB14Mc4T5sD8DqA5au12v3bmBuRa+rqpwlW
IehFFD3p1zM1z1DmLekQYIY74vpEmjHVH5rA0tehIByjViHFXwuZKZvnAoGAQZQd
e1i97ZTIYgwUlpeH758O+a6pd+19MQgmLuSwv8vZvj4LvBStFijFOE9aShwQ0iyz
v6vln8fGuwK6QnOO/B0Oc9M/TaOwnNDNkN64DQ8dDWyJ4sOtFmxIJTO78e6SPZZc
hsoRaOkC3x/Lztua7LISQlZ7FpYhqwx9ZAGcy2kCgYAILVNi5GiOGHpH0YApBD7m
njrLWgQcV84jPKUGV8pK3NnXi72v9xuPw0jKEZ0bEqbUmpFytwoqO86HyhqgIVmT
5mqgo6ZEdH7HdErSQ214Jn+mJGOKPL5ATV25saedad+ZwdWFv5iM9qVHd0oElani
9EKE9krwYlhyzUEyB74aDA==
-----END PRIVATE KEY-----
"#;

pub(crate) const TEST_CLIENT_LOCAL_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDKzCCAhOgAwIBAgIUVnjkCKXzQiyHyEsQ3ey4lgWkrTAwDQYJKoZIhvcNAQEL
BQAwHDEaMBgGA1UEAwwRdW5kZXJ3cmFwLXRlc3QtY2EwHhcNMjYwNDE2MDAzNzAy
WhcNMzYwNDEzMDAzNzAyWjAaMRgwFgYDVQQDDA91bmRlcndyYXAtbG9jYWwwggEi
MA0GCSqGSIb3DQEBAQUAA4IBDwAwggEKAoIBAQDcJDP5K69TcG9+uHM7q+xG7p0c
7Cgex8XYVs3yFDpHk6UOG/JqL0bKa32rhO7LR8VxPprIyqV24LB8vTi52A2/oA/d
ZiVouv9bkCiBOun0XDQAHveocrIYnhjvq8qrpdfFdZ+2oB/V78Mbf6FaNMJxvvUN
hCRXh/hstwlHVACJFX0MlGU/3WAhpRF09QjIfxLUqm1t5Sd9Qg0a2IsX+qrFhDiu
35anBoCx0x9AJl6yO40KgoDZ7iDmWLw55T2GVnSQEdTelxzsf9jFLjEzm56mymR+
DGWdMNPEKY6wyvuJClEYLEsHaNMfTi9Jp+9FgH5spuy7E9UqpqeciYAHlDrxAgMB
AAGjZzBlMBMGA1UdJQQMMAoGCCsGAQUFBwMCMA4GA1UdDwEB/wQEAwIFoDAdBgNV
HQ4EFgQUrZfzoVv+iUwAlCedd1/ancfUi7YwHwYDVR0jBBgwFoAUlScjoHF7mIGw
Qy61SLb9isP1dd4wDQYJKoZIhvcNAQELBQADggEBAMj5BtbOalwYWAi9HQt0JNPq
NtRObdNx6Yg2uZWYAyhVXnhLerZ82hWagK/oH0KCgKFCSK140oZS/Nt7xtQ9YJXs
g5WyGnsteF3QZ9cuUc+0te4eMOmXRMch8HtxyD1sCuO/wyKhtQSCO+OQGNvsEnKa
1rl3PFTjG3SE5ldDwRjGSlrf9+6uf2OcqJBLaL4dcOJ0Fsrf0gkkPpbrC5hJdNSo
Q+0W8oyLdzgmGNMRY4vvhu4cjYkbPLgrNS3fQLnnom3Jv5GzO3M2ifS9mkj249Bu
cFuCWqBWazuUyiSP6kQz7Ad4/9qxEu4c8Hv3Ecc5XuQDpl4Rxox+j/7lUrthzrI=
-----END CERTIFICATE-----
"#;

pub(crate) const TEST_CLIENT_LOCAL_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDcJDP5K69TcG9+
uHM7q+xG7p0c7Cgex8XYVs3yFDpHk6UOG/JqL0bKa32rhO7LR8VxPprIyqV24LB8
vTi52A2/oA/dZiVouv9bkCiBOun0XDQAHveocrIYnhjvq8qrpdfFdZ+2oB/V78Mb
f6FaNMJxvvUNhCRXh/hstwlHVACJFX0MlGU/3WAhpRF09QjIfxLUqm1t5Sd9Qg0a
2IsX+qrFhDiu35anBoCx0x9AJl6yO40KgoDZ7iDmWLw55T2GVnSQEdTelxzsf9jF
LjEzm56mymR+DGWdMNPEKY6wyvuJClEYLEsHaNMfTi9Jp+9FgH5spuy7E9Uqpqec
iYAHlDrxAgMBAAECggEABExQZ7a1ss7K3X31UYKf/4tKfeFiP+YZf7mjphylHedF
AWlUJoS3aI24V5pGBYnoYed1kxKqT/+vcHMLFc1RKObzklh7hd/DSzbNhKbpYRYR
BZjVL+iz35lMHjdT4t2vQJ2lHXYjQvx+OiXn+QR3UKxYWWAFa+BVrk01URtDsqlz
hB2LutpbqjGBTNmw6MBjewdoKHnfQr/+gXpP6h1o4pbi2Ldf9gQXjI/xaor1DcvL
e6mEQ0w6ypeFOZl/FlSlsHD3k93Ks3P32vnCteTHYKdk2/gZUhnDosb9PSq6MXLC
FZsf24y0JJSoVM/5ynx5SM0ZiM78pTm5LvEdlaXdYQKBgQD9i/oqGYXpWIHZu6v8
vtYr4Cmmyg51jY4E4omJhfzibbwB+MvEA3Ssa0/0bBpPLQYKiu9uJDCEQii8pJkD
U6n48OqfqFJ76IrsLsrEcO74cP1Y1UI+PwietYYIrTvUARwPZajNd/I0pcB1Fhj+
VzdSfR5i30iLCVA4W7cYANunYQKBgQDeRXt9LhoKGcstJilr+2U1JfeqqpW87Z0y
ukOy5T4RKwACEqCV4KR1xlhbYp8EOnODO3BTgbY91QeCN2JFwvBNVpJ0B32fWno7
qcjI5gnjrqAddI4gE01PZTflXYJaYDB5BLZj4RqfxLTrvKGdRbZ3pOGKEIIC4Dqn
47fBJDqNkQKBgQDmyXEuvQi9fEjF7iQA7Zx5V5PSM/2xVuY84A+dVI75dDU7/kFL
7HFZOF0Q+Q2BoFPtaMXEFcDrwmTeQVzeQFtpM7H9HfPmwacikP0Ujk+XaRHSuFuR
2UJ5ZHXalATvVws5oKlkTZf0ACQM785URQMgXK86BFA97n5Mrn8cpDtMQQKBgAUe
KnK+bgEPiPsYJtoCptjrb+FxHhOEkRsK6OYkmyJAEDb8ANCSpJbFo2EvSyQIyIIc
C0EQ+oY/8nVQGyYqD+hn9uVPCthxd+EJzsK5cT7Z1Xv4JBGfACe01+2jEo5MKrGO
pzhzhW5qVBQdescpRMNKe5r3OFcinJcjCicbU3HRAoGBAJyyj47yD7wmCTYjI9lm
rrcOpIyQtPhY4bEQjdcW7OLNkfLFlM0iSNgusC871ul8MGh2bRV576UzBu9GWkLq
yagHeu/N+0B5jZ7TUD3FNiTWHDglKbkMSi7rbDUFQZDOKG0A+Y5B2qobPSEoTw03
n+Q8b5k7ohmMbkOzzdUIrM8L
-----END PRIVATE KEY-----
"#;

pub(crate) const TEST_CLIENT_TUNNEL_PEM: &str = r#"-----BEGIN CERTIFICATE-----
MIIDLDCCAhSgAwIBAgIUVnjkCKXzQiyHyEsQ3ey4lgWkrTEwDQYJKoZIhvcNAQEL
BQAwHDEaMBgGA1UEAwwRdW5kZXJ3cmFwLXRlc3QtY2EwHhcNMjYwNDE2MDAzNzAy
WhcNMzYwNDEzMDAzNzAyWjAbMRkwFwYDVQQDDBB1bmRlcndyYXAtdHVubmVsMIIB
IjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA6Zlv1zO0UhnP8lTcKPu72yk0
O3kQWwAEMU5XSP84Zyx1Rob4f70ukrkGrSEdaGJgYkr4+ddDvK9g4brDrGz2a/67
4MuG/O3Qnqjf4G0VR4w80cr9/f0p3ePsGFUDMTZTVBKyUgKstm9HWT5tJYtFaS/l
aSuZc3uHlmiDyxo9nPHA2P6T8oQHd4rhnxpNCHCThGDkqLtZPab8iofP7HlXc5q/
GLP/mR8myopuf6dB3cZIUl1T9sd2QhOy/EKhtKrDvB+VA7BYAYFG/eJxpcDw1n0C
uaGKKGhlKN4nDwvsDjvuSIyrD9K3IKBdUHQw7qCAx0TSWgcrikp9Cd+HmCyycwID
AQABo2cwZTATBgNVHSUEDDAKBggrBgEFBQcDAjAOBgNVHQ8BAf8EBAMCBaAwHQYD
VR0OBBYEFBoz4yFrIHRXhK4m14r1xcFknR3qMB8GA1UdIwQYMBaAFJUnI6Bxe5iB
sEMutUi2/YrD9XXeMA0GCSqGSIb3DQEBCwUAA4IBAQCAu6E2ovw9PMd76NoYByeB
oLomNEt3MqzuDwcbC51lqcoxEUd27Lwaj1f7QJhSotRHQXyBPXDLx+RLg5NqcDDu
KalPgx0lwgfWl7YIEwYNSgPDKBKTlBYmKAzwpotSkvcUG+A4CB0cW9vQMYVs7N5K
y2N9mjRjJ5YnEZYR/MPAoN1KVrkbFVXWlrRU/GQby+d1cdxjzv50EF3Xrp9AASJH
HC+swIzw4x0XvDesFyLWoPs/oqzYCnuQ3iO6AnIdSXrG19V3C14WdWnzkaSTkBR5
z4+5Ls2wVcKKSFUKzG1NlD/sKY+2KGbF7iiN1TvlCo/bjxtNlCyL7i2tNRBJqPwD
-----END CERTIFICATE-----
"#;

pub(crate) const TEST_CLIENT_TUNNEL_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvwIBADANBgkqhkiG9w0BAQEFAASCBKkwggSlAgEAAoIBAQDpmW/XM7RSGc/y
VNwo+7vbKTQ7eRBbAAQxTldI/zhnLHVGhvh/vS6SuQatIR1oYmBiSvj510O8r2Dh
usOsbPZr/rvgy4b87dCeqN/gbRVHjDzRyv39/Snd4+wYVQMxNlNUErJSAqy2b0dZ
Pm0li0VpL+VpK5lze4eWaIPLGj2c8cDY/pPyhAd3iuGfGk0IcJOEYOSou1k9pvyK
h8/seVdzmr8Ys/+ZHybKim5/p0HdxkhSXVP2x3ZCE7L8QqG0qsO8H5UDsFgBgUb9
4nGlwPDWfQK5oYooaGUo3icPC+wOO+5IjKsP0rcgoF1QdDDuoIDHRNJaByuKSn0J
34eYLLJzAgMBAAECggEAEOJvTA3HrduBxVFO7INti6TJyrawcPNqWioAcx1HdWWo
QGXHFmyVLjPDgMYQ+xeJQfGLV3uwwLWoeghm1lK16Q5rhA8R/bwuxRkNvYoE7Lv9
NWjHvREMcqZjzUpvxaHYmg4T54ub8j/O12uEth7/8F9u5JArMkaugTeitw55f5ty
aiCLj/cba0p5sayq1Q77AYeGQze+dOheltOHlpLFsRrPY23tcd9p6S5RxSODhOxJ
Jm1OcGALlcig8XOnHsLBqdQQyDu7N7KISe4eVDl2vrzxi/T/0HhtBSOGZlm0F+ex
D2jul2C9kJYUhtJG0Pp8NC+91XRIEz3Vl3KDv277EQKBgQD5iL0oF8+DW7m36MjU
LjsL7hQqBvGl8+J/dly/XHEL3g3oPkqJn+Bmzw+O58aoZq6L612p8eeinR7DLhJf
W384lo8wyvW11SZfabtGlfv2rNBPEFkTqf8K2IlA62rsmkjk2JpVXPRfow4341JE
vGMAC0MUlbr/Vw8h9f7uz12uOwKBgQDvpv8FCaHL6OohptevxkcJNTXhVyeXCuzi
uPAU7zxunEdCfkCOrpDaNwXEAbtGRR0i4fmeXPAKq0SqFghz7+bVGXkewrnoFM5X
JttVFpcK57STno2Yg+kw8M8Pbzqd+yz48uRyG79TCpZa/OWFxYtQffgDM3UAErP8
645Hgc+xKQKBgQDL8Vj/+q3sYzxftg7lc79ombW7P5+ijbiRinB1Kh1uoJbnOHn9
D0D9ZcAGgSIZIosH6l7L9dzmR9oLlY0IiKRq4Iq5SzgU29eX5bddElozGrzJ0cer
zaWX/K/48i+anuJyvyEsIWPNAlwti0oCyaF0OMAjg9iotkO08vPfxHWBXwKBgQDr
1L4oSu45Jx8ftnuRseC9ous+sPF4pXuvMKaxMyuY/ZjySiNlRUuuMW8vDzUnVWPy
q8VknbhqB1guuDfhPzc8hTC3oJApXjAfe1FjTjOcu57rlZ9pcoXLb22TlrVsOXxs
DznXQI7qPQTwzOVg2qDZuNyS6dQ9GiqcvYDpxUiI4QKBgQCWXx5sy7sQOKV8LLI1
s2nWK9tXXBiCDkLj4uWVVirM5eZaNBc/3LhzMWNrFvjOkewa10lyGShKbuWBXGyJ
zudEDAqozu2dZ/IIyOA3kMXlCRCEbhMGJB8M72nGJ2ARtqk9rklxz3YBUwk8s/6r
Nlst7NiyDJwGjvGceYEfzrQieg==
-----END PRIVATE KEY-----
"#;

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

fn test_pki_paths(base: &std::path::Path, client_name: &str) -> Result<TestPkiPaths> {
    let pki_dir = base.join("pki");
    std::fs::create_dir_all(&pki_dir)?;

    let ca_cert = pki_dir.join("ca.pem");
    let server_cert = pki_dir.join("server.pem");
    let server_key = pki_dir.join("server.key");
    std::fs::write(&ca_cert, TEST_CA_PEM.as_bytes())?;
    std::fs::write(&server_cert, TEST_SERVER_PEM.as_bytes())?;
    std::fs::write(&server_key, TEST_SERVER_KEY.as_bytes())?;

    let (client_cert, client_key, client_cert_bytes, client_key_bytes): (&str, &str, &[u8], &[u8]) =
        match client_name {
            "local" => (
                "client-local.pem",
                "client-local.key",
                TEST_CLIENT_LOCAL_PEM.as_bytes(),
                TEST_CLIENT_LOCAL_KEY.as_bytes(),
            ),
            "tunnel" => (
                "client-tunnel.pem",
                "client-tunnel.key",
                TEST_CLIENT_TUNNEL_PEM.as_bytes(),
                TEST_CLIENT_TUNNEL_KEY.as_bytes(),
            ),
            other => anyhow::bail!("unknown test client name '{other}'"),
        };
    let client_cert = pki_dir.join(client_cert);
    let client_key = pki_dir.join(client_key);
    std::fs::write(&client_cert, client_cert_bytes)?;
    std::fs::write(&client_key, client_key_bytes)?;

    Ok(TestPkiPaths {
        ca_cert,
        server_cert,
        server_key,
        client_cert,
        client_key,
    })
}

fn pick_unused_port() -> Result<u16> {
    use std::sync::atomic::{AtomicU16, Ordering};
    static NEXT_PORT: AtomicU16 = AtomicU16::new(43000);
    let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
    anyhow::ensure!(port < 60000, "ran out of test ports");
    Ok(port)
}

async fn wait_for_admin(url: &str) -> Result<()> {
    let client = reqwest::Client::new();
    for _ in 0..100 {
        if let Ok(response) = client.get(format!("{url}/v1/admin/stores")).send().await
            && response.status().is_success()
        {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    anyhow::bail!("admin API never became ready at {url}")
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
