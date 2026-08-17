use super::*;
use std::io::{Read, Write};

mod concurrency;

const TEST_CA_PEM: &str = include_str!("../../../integration-tests/testdata/pki/ca.pem");
const TEST_SERVER_PEM: &str = include_str!("../../../integration-tests/testdata/pki/server.pem");
const TEST_SERVER_KEY: &str = include_str!("../../../integration-tests/testdata/pki/server.key");
const TEST_CLIENT_PEM: &str =
    include_str!("../../../integration-tests/testdata/pki/client-local.pem");
const TEST_CLIENT_KEY: &str =
    include_str!("../../../integration-tests/testdata/pki/client-local.key");

#[derive(Debug, PartialEq, Eq)]
enum BackendCaPolicyOperation {
    BuiltInRoots(bool),
    AddConfiguredRoot,
}

#[derive(Default)]
struct RecordingBackendCaPolicyTarget {
    operations: Vec<BackendCaPolicyOperation>,
}

impl BackendCaPolicyTarget for RecordingBackendCaPolicyTarget {
    fn tls_built_in_root_certs(mut self, enabled: bool) -> Self {
        self.operations
            .push(BackendCaPolicyOperation::BuiltInRoots(enabled));
        self
    }

    fn add_backend_root_certificate(mut self, _certificate: Certificate) -> Self {
        self.operations
            .push(BackendCaPolicyOperation::AddConfiguredRoot);
        self
    }
}

#[test]
fn backend_ca_policy_configured_mode_disables_built_ins() {
    let certificate = Certificate::from_pem(TEST_CA_PEM.as_bytes()).unwrap();
    let recorder =
        apply_backend_ca_policy(RecordingBackendCaPolicyTarget::default(), Some(certificate));

    assert_eq!(
        recorder.operations,
        vec![
            BackendCaPolicyOperation::BuiltInRoots(false),
            BackendCaPolicyOperation::AddConfiguredRoot,
        ]
    );
}

#[test]
fn backend_ca_policy_default_mode_keeps_built_ins() {
    let recorder = apply_backend_ca_policy(RecordingBackendCaPolicyTarget::default(), None);

    assert_eq!(
        recorder.operations,
        vec![BackendCaPolicyOperation::BuiltInRoots(true)]
    );
}

#[test]
fn open_fails_immediately_on_bad_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let ca = dir.path().join("ca.pem");
    let identity = dir.path().join("client.identity.pem");
    for path in [&ca, &identity] {
        std::fs::File::create(path)
            .unwrap()
            .write_all(b"not-a-cert")
            .unwrap();
    }
    let result = BackendClient::open(&BackendServiceConfig {
        url: "https://localhost:0".to_string(),
        ca_file: Some(ca),
        client_identity_file: Some(identity),
    });
    assert!(result.is_err());
    let msg = format!("{:#}", result.err().unwrap());
    assert!(
        msg.contains("parsing backend CA PEM") || msg.contains("parsing backend identity PEM"),
        "unexpected error: {msg}",
    );
}

#[test]
fn open_rejects_missing_client_identity() {
    let result = BackendClient::open(&BackendServiceConfig {
        url: "https://backend.example.internal".to_string(),
        ca_file: None,
        client_identity_file: None,
    });
    assert!(
        format!("{:#}", result.err().unwrap()).contains("client_identity_file"),
        "expected client_identity_file error",
    );
}

#[test]
fn backend_url_rejection_precedes_all_credential_file_reads() {
    let result = BackendClient::open(&BackendServiceConfig {
        url: "http://plaintext.example.invalid".to_string(),
        ca_file: Some("/unusable/ca.pem".into()),
        client_identity_file: Some("/unusable/client.identity.pem".into()),
    });
    assert_eq!(
        result.err().unwrap().to_string(),
        "backend.url must be an absolute HTTPS URL with a host"
    );
}

#[test]
fn backend_url_endpoint_preserves_non_root_base_path_and_trailing_slash() {
    assert_eq!(
        endpoint(
            "https://proxy.example.internal/base/runtime/",
            "/v1/wrappers"
        ),
        "https://proxy.example.internal/base/runtime/v1/wrappers"
    );
    assert_eq!(
        endpoint("https://backend.example.internal", "/v1/wrappers"),
        "https://backend.example.internal/v1/wrappers"
    );
}

fn write_fixture(path: &std::path::Path, contents: &str) {
    std::fs::write(path, contents.as_bytes()).unwrap();
}

fn write_identity_fixture(path: &std::path::Path, cert: &str, key: &str) {
    let mut identity = cert.as_bytes().to_vec();
    identity.extend_from_slice(key.as_bytes());
    std::fs::write(path, identity).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn https_only_redirect_never_connects_to_plaintext_trap() {
    use rustls::pki_types::pem::PemObject;
    use std::sync::Arc;

    let _ = rustls::crypto::ring::default_provider().install_default();
    let trap = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    trap.set_nonblocking(true).unwrap();
    let trap_address = trap.local_addr().unwrap();
    let (trap_stop_tx, trap_stop_rx) = std::sync::mpsc::channel();

    let tls_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    tls_listener.set_nonblocking(true).unwrap();
    let tls_address = tls_listener.local_addr().unwrap();
    let certificate =
        rustls::pki_types::CertificateDer::from_pem_slice(TEST_SERVER_PEM.as_bytes()).unwrap();
    let private_key =
        rustls::pki_types::PrivateKeyDer::from_pem_slice(TEST_SERVER_KEY.as_bytes()).unwrap();
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], private_key)
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let ca = dir.path().join("ca.pem");
    let identity = dir.path().join("client.identity.pem");
    write_fixture(&ca, TEST_CA_PEM);
    write_identity_fixture(&identity, TEST_CLIENT_PEM, TEST_CLIENT_KEY);
    let client = BackendClient::open(&BackendServiceConfig {
        url: format!("https://127.0.0.1:{}/proxy-base/", tls_address.port()),
        ca_file: Some(ca),
        client_identity_file: Some(identity),
    })
    .unwrap();

    let trap_server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match trap.accept() {
                Ok((mut stream, _)) => {
                    let timeout = Some(std::time::Duration::from_secs(1));
                    stream.set_read_timeout(timeout).unwrap();
                    stream.set_write_timeout(timeout).unwrap();
                    let mut request = [0u8; 4096];
                    let read = stream.read(&mut request).unwrap();
                    assert!(read > 0);
                    stream
                        .write_all(
                            b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .unwrap();
                    stream.flush().unwrap();
                    return 1usize;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("plaintext redirect trap accept failed: {error}"),
            }

            match trap_stop_rx.try_recv() {
                Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected) => return 0,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
            assert!(
                std::time::Instant::now() < deadline,
                "plaintext redirect trap exceeded its deadline"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    let tls_server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let stream = loop {
            match tls_listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("HTTPS redirect server accept failed: {error}"),
            }
            assert!(
                std::time::Instant::now() < deadline,
                "HTTPS redirect server exceeded its accept deadline"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let timeout = Some(std::time::Duration::from_secs(5));
        stream.set_read_timeout(timeout).unwrap();
        stream.set_write_timeout(timeout).unwrap();
        let connection = rustls::ServerConnection::new(Arc::new(server_config)).unwrap();
        let mut tls = rustls::StreamOwned::new(connection, stream);
        let mut request = [0u8; 4096];
        let read = tls.read(&mut request).unwrap();
        assert!(read > 0);
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: http://{trap_address}/plaintext-trap\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        tls.write_all(response.as_bytes()).unwrap();
        tls.flush().unwrap();
    });

    let register_result = client
        .register(RuntimeWrapperRegisterRequest { wrappers: vec![] })
        .await;
    let _ = trap_stop_tx.send(());
    let tls_result = tls_server.join();
    let trap_result = trap_server.join();

    tls_result.unwrap();
    let plaintext_connections = trap_result.unwrap();
    assert_eq!(
        plaintext_connections, 0,
        "plaintext redirect trap unexpectedly accepted a connection"
    );
    let error = register_result.expect_err("HTTPS-only client must reject HTTP redirect");
    assert!(
        format!("{error:#}").contains("sending backend register request"),
        "unexpected redirect error: {error:#}"
    );
}
