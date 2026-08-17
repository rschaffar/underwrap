use crate::harness::{
    TEST_CA_SELECTED_EXCLUSIVE_PEM, TEST_CA_UNSELECTED_PEM, TEST_CLIENT_LOCAL_KEY,
    TEST_CLIENT_LOCAL_PEM, TEST_SERVER_SELECTED_EXCLUSIVE_KEY, TEST_SERVER_SELECTED_EXCLUSIVE_PEM,
    TEST_SERVER_UNSELECTED_CA_KEY, TEST_SERVER_UNSELECTED_CA_PEM, TEST_SERVER_WRONG_HOST_KEY,
    TEST_SERVER_WRONG_HOST_PEM,
};
use rustls::pki_types::pem::PemObject;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use underwrap_protocol::config::BackendServiceConfig;
use underwrap_protocol::service::{RuntimeWrapperRegisterRequest, RuntimeWrapperRegisterResponse};
use underwrapd::backend_client::BackendClient;

const PROBE_DEADLINE: Duration = Duration::from_secs(5);
const COORDINATION_DEADLINE: Duration = Duration::from_secs(8);
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(1);
const SOCKET_WAIT: Duration = Duration::from_millis(50);
const COORDINATION_POLL: Duration = Duration::from_millis(5);
const MAX_HTTP_REQUEST_BYTES: usize = 16 * 1024;

#[derive(Debug)]
enum ProbeObservation {
    HttpRequest(Vec<u8>),
    RejectedBeforeHttp { alert: rustls::AlertDescription },
    PartialHttp { bytes: usize, error: String },
    HarnessFailure(String),
}

struct CaProbe {
    address: SocketAddr,
    stop: mpsc::Sender<()>,
    observation: mpsc::Receiver<ProbeObservation>,
    handle: thread::JoinHandle<()>,
}

#[derive(Debug)]
struct ClientFailure {
    chain: Vec<String>,
}

impl ClientFailure {
    fn harness(message: impl Into<String>) -> Self {
        Self {
            chain: vec![message.into()],
        }
    }

    fn from_anyhow(error: anyhow::Error) -> Self {
        Self {
            chain: error.chain().map(ToString::to_string).collect(),
        }
    }
}

#[derive(Debug)]
struct ProbeRun {
    client_result: Result<Result<RuntimeWrapperRegisterResponse, ClientFailure>, String>,
    observation: Result<ProbeObservation, String>,
    client_joined: bool,
    server_joined: bool,
}

fn spawn_ca_probe(certificate_pem: &str, private_key_pem: &str) -> Result<CaProbe, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let certificate = rustls::pki_types::CertificateDer::from_pem_slice(certificate_pem.as_bytes())
        .map_err(|error| error.to_string())?;
    let private_key = rustls::pki_types::PrivateKeyDer::from_pem_slice(private_key_pem.as_bytes())
        .map_err(|error| error.to_string())?;
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], private_key)
        .map_err(|error| error.to_string())?;
    let (stop_tx, stop_rx) = mpsc::channel();
    let (observation_tx, observation_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let observation = serve_ca_probe(listener, Arc::new(server_config), stop_rx);
        let _ = observation_tx.send(observation);
    });

    Ok(CaProbe {
        address,
        stop: stop_tx,
        observation: observation_rx,
        handle,
    })
}

fn stop_or_deadline(
    stop: &mpsc::Receiver<()>,
    deadline: Instant,
    phase: &str,
) -> Option<ProbeObservation> {
    match stop.try_recv() {
        Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {
            return Some(ProbeObservation::HarnessFailure(format!(
                "probe stopped during {phase}"
            )));
        }
        Err(mpsc::TryRecvError::Empty) => {}
    }
    if Instant::now() >= deadline {
        return Some(ProbeObservation::HarnessFailure(format!(
            "absolute probe deadline elapsed during {phase}"
        )));
    }
    None
}

fn is_socket_wait(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

fn serve_http_response(
    connection: &mut rustls::ServerConnection,
    stream: &mut TcpStream,
    stop: &mpsc::Receiver<()>,
    deadline: Instant,
    request: Vec<u8>,
) -> ProbeObservation {
    let body = br#"{"outcome":"ok","auto_created_resources":[],"auto_created_pairs":[]}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if let Err(error) = connection
        .writer()
        .write_all(response.as_bytes())
        .and_then(|()| connection.writer().write_all(body))
    {
        return ProbeObservation::PartialHttp {
            bytes: request.len(),
            error: format!("buffering HTTP response: {error}"),
        };
    }

    while connection.wants_write() {
        if let Some(failure) = stop_or_deadline(stop, deadline, "HTTP response write") {
            return failure;
        }
        match connection.write_tls(stream) {
            Ok(0) => {
                return ProbeObservation::PartialHttp {
                    bytes: request.len(),
                    error: "TLS response write made no progress".to_string(),
                };
            }
            Ok(_) => {}
            Err(error) if is_socket_wait(&error) => {}
            Err(error) => {
                return ProbeObservation::PartialHttp {
                    bytes: request.len(),
                    error: format!("writing HTTP response: {error}"),
                };
            }
        }
    }
    ProbeObservation::HttpRequest(request)
}

fn serve_accepted_connection(
    mut stream: TcpStream,
    server_config: Arc<rustls::ServerConfig>,
    stop: &mpsc::Receiver<()>,
) -> ProbeObservation {
    if let Err(error) = stream.set_read_timeout(Some(SOCKET_WAIT)) {
        return ProbeObservation::HarnessFailure(format!("setting read deadline: {error}"));
    }
    if let Err(error) = stream.set_write_timeout(Some(SOCKET_WAIT)) {
        return ProbeObservation::HarnessFailure(format!("setting write deadline: {error}"));
    }
    let mut connection = match rustls::ServerConnection::new(server_config) {
        Ok(connection) => connection,
        Err(error) => {
            return ProbeObservation::HarnessFailure(format!("creating TLS connection: {error}"));
        }
    };
    let deadline = Instant::now() + PROBE_DEADLINE;
    let mut request = Vec::new();
    let mut buffer = [0u8; 2048];

    loop {
        if let Some(failure) = stop_or_deadline(stop, deadline, "TLS request read") {
            return failure;
        }

        if connection.wants_write() {
            match connection.write_tls(&mut stream) {
                Ok(0) => {
                    return ProbeObservation::HarnessFailure(
                        "TLS handshake write made no progress".to_string(),
                    );
                }
                Ok(_) => {}
                Err(error) if is_socket_wait(&error) => continue,
                Err(error) => {
                    return ProbeObservation::HarnessFailure(format!(
                        "writing TLS handshake: {error}"
                    ));
                }
            }
        }

        match connection.read_tls(&mut stream) {
            Ok(0) => {
                return if request.is_empty() {
                    ProbeObservation::HarnessFailure(
                        "peer terminated TLS without a certificate-authentication alert"
                            .to_string(),
                    )
                } else {
                    ProbeObservation::PartialHttp {
                        bytes: request.len(),
                        error: "connection ended before complete HTTP headers".to_string(),
                    }
                };
            }
            Ok(_) => {}
            Err(error) if is_socket_wait(&error) => continue,
            Err(error) => {
                return if request.is_empty() {
                    ProbeObservation::HarnessFailure(format!("reading TLS transport: {error}"))
                } else {
                    ProbeObservation::PartialHttp {
                        bytes: request.len(),
                        error: format!("TLS transport ended after partial decrypted HTTP: {error}"),
                    }
                };
            }
        }

        match connection.process_new_packets() {
            Ok(_) => {}
            Err(rustls::Error::AlertReceived(alert)) if request.is_empty() => {
                return match alert {
                    rustls::AlertDescription::UnknownCA
                    | rustls::AlertDescription::BadCertificate => {
                        ProbeObservation::RejectedBeforeHttp { alert }
                    }
                    _ => ProbeObservation::HarnessFailure(format!(
                        "unexpected peer TLS alert before HTTP: {alert:?}"
                    )),
                };
            }
            Err(error) if request.is_empty() => {
                return ProbeObservation::HarnessFailure(format!(
                    "processing TLS before HTTP: {error}"
                ));
            }
            Err(error) => {
                return ProbeObservation::PartialHttp {
                    bytes: request.len(),
                    error: format!("processing TLS after partial decrypted HTTP: {error}"),
                };
            }
        }

        loop {
            match connection.reader().read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    request.extend_from_slice(&buffer[..read]);
                    if request.len() > MAX_HTTP_REQUEST_BYTES {
                        return ProbeObservation::PartialHttp {
                            bytes: request.len(),
                            error: "HTTP request exceeded probe limit".to_string(),
                        };
                    }
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        return serve_http_response(
                            &mut connection,
                            &mut stream,
                            stop,
                            deadline,
                            request,
                        );
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    return if request.is_empty() {
                        ProbeObservation::HarnessFailure(format!(
                            "reading decrypted TLS data: {error}"
                        ))
                    } else {
                        ProbeObservation::PartialHttp {
                            bytes: request.len(),
                            error: format!("reading partial decrypted HTTP: {error}"),
                        }
                    };
                }
            }
        }
    }
}

fn serve_ca_probe(
    listener: TcpListener,
    server_config: Arc<rustls::ServerConfig>,
    stop: mpsc::Receiver<()>,
) -> ProbeObservation {
    let accept_deadline = Instant::now() + PROBE_DEADLINE;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return serve_accepted_connection(stream, server_config, &stop),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => {
                return ProbeObservation::HarnessFailure(format!("accept failed: {error}"));
            }
        }
        if let Some(failure) = stop_or_deadline(&stop, accept_deadline, "connection accept") {
            return failure;
        }
        thread::sleep(COORDINATION_POLL);
    }
}

fn invoke_backend_register(
    address: SocketAddr,
) -> Result<RuntimeWrapperRegisterResponse, ClientFailure> {
    let directory = tempfile::tempdir()
        .map_err(|error| ClientFailure::harness(format!("creating fixture directory: {error}")))?;
    let ca_file = directory.path().join("selected-ca.pem");
    let client_identity_file = directory.path().join("client.identity.pem");
    std::fs::write(&ca_file, TEST_CA_SELECTED_EXCLUSIVE_PEM)
        .map_err(|error| ClientFailure::harness(format!("writing fixture CA: {error}")))?;
    let mut client_identity = TEST_CLIENT_LOCAL_PEM.as_bytes().to_vec();
    client_identity.extend_from_slice(TEST_CLIENT_LOCAL_KEY.as_bytes());
    std::fs::write(&client_identity_file, client_identity)
        .map_err(|error| ClientFailure::harness(format!("writing fixture identity: {error}")))?;

    let client = BackendClient::open(&BackendServiceConfig {
        url: format!("https://127.0.0.1:{}", address.port()),
        ca_file: Some(ca_file),
        client_identity_file: Some(client_identity_file),
    })
    .map_err(ClientFailure::from_anyhow)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            ClientFailure::harness(format!("building backend client runtime: {error}"))
        })?;
    runtime
        .block_on(client.register(RuntimeWrapperRegisterRequest { wrappers: vec![] }))
        .map_err(ClientFailure::from_anyhow)
}

fn collect_thread_results(
    client_rx: &mpsc::Receiver<Result<RuntimeWrapperRegisterResponse, ClientFailure>>,
    observation_rx: &mpsc::Receiver<ProbeObservation>,
    client_result: &mut Option<
        Result<Result<RuntimeWrapperRegisterResponse, ClientFailure>, String>,
    >,
    observation: &mut Option<Result<ProbeObservation, String>>,
) {
    if client_result.is_none() {
        match client_rx.try_recv() {
            Ok(result) => *client_result = Some(Ok(result)),
            Err(mpsc::TryRecvError::Disconnected) => {
                *client_result = Some(Err(
                    "backend client thread exited without a result".to_string()
                ));
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    if observation.is_none() {
        match observation_rx.try_recv() {
            Ok(result) => *observation = Some(Ok(result)),
            Err(mpsc::TryRecvError::Disconnected) => {
                *observation = Some(Err(
                    "CA probe thread exited without an observation".to_string()
                ));
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

fn wait_for_thread_results(
    deadline: Instant,
    client_rx: &mpsc::Receiver<Result<RuntimeWrapperRegisterResponse, ClientFailure>>,
    observation_rx: &mpsc::Receiver<ProbeObservation>,
    client_result: &mut Option<
        Result<Result<RuntimeWrapperRegisterResponse, ClientFailure>, String>,
    >,
    observation: &mut Option<Result<ProbeObservation, String>>,
) {
    while Instant::now() < deadline && (client_result.is_none() || observation.is_none()) {
        collect_thread_results(client_rx, observation_rx, client_result, observation);
        if client_result.is_none() || observation.is_none() {
            thread::sleep(COORDINATION_POLL);
        }
    }
    collect_thread_results(client_rx, observation_rx, client_result, observation);
}

fn run_backend_ca_probe(certificate_pem: &str, private_key_pem: &str) -> ProbeRun {
    let probe = match spawn_ca_probe(certificate_pem, private_key_pem) {
        Ok(probe) => probe,
        Err(error) => {
            return ProbeRun {
                client_result: Err(format!("starting CA probe: {error}")),
                observation: Err("CA probe did not start".to_string()),
                client_joined: true,
                server_joined: true,
            };
        }
    };
    let CaProbe {
        address,
        stop,
        observation: observation_rx,
        handle: server_handle,
    } = probe;
    let (client_tx, client_rx) = mpsc::channel();
    let client_handle = thread::spawn(move || {
        let _ = client_tx.send(invoke_backend_register(address));
    });
    let mut client_result = None;
    let mut observation = None;

    wait_for_thread_results(
        Instant::now() + COORDINATION_DEADLINE,
        &client_rx,
        &observation_rx,
        &mut client_result,
        &mut observation,
    );

    let _ = stop.send(());
    wait_for_thread_results(
        Instant::now() + SHUTDOWN_DEADLINE,
        &client_rx,
        &observation_rx,
        &mut client_result,
        &mut observation,
    );

    let server_joined = server_handle.join().is_ok();
    let client_joined = client_result.is_some() && client_handle.join().is_ok();

    ProbeRun {
        client_result: client_result
            .unwrap_or_else(|| Err("backend client thread missed shutdown deadline".to_string())),
        observation: observation
            .unwrap_or_else(|| Err("CA probe thread missed shutdown deadline".to_string())),
        client_joined,
        server_joined,
    }
}

fn assert_probe_threads_joined(run: &ProbeRun) {
    assert!(
        run.server_joined,
        "CA probe server thread did not finish boundedly: {run:?}"
    );
    assert!(
        run.client_joined,
        "backend client thread did not finish boundedly: {run:?}"
    );
}

#[test]
fn backend_ca_selected_authority_is_accepted() {
    let run = run_backend_ca_probe(
        TEST_SERVER_SELECTED_EXCLUSIVE_PEM,
        TEST_SERVER_SELECTED_EXCLUSIVE_KEY,
    );
    assert_probe_threads_joined(&run);

    let response = run
        .client_result
        .as_ref()
        .expect("backend client result arrived")
        .as_ref()
        .expect("selected server authority must authenticate");
    assert!(
        matches!(response, RuntimeWrapperRegisterResponse::Ok { .. }),
        "unexpected register response: {response:?}"
    );
    match run.observation.as_ref().expect("probe observation arrived") {
        ProbeObservation::HttpRequest(request) => assert!(
            request.starts_with(b"POST /v1/wrappers HTTP/1.1\r\n"),
            "unexpected decrypted request: {}",
            String::from_utf8_lossy(request)
        ),
        ProbeObservation::HarnessFailure(error) => {
            panic!("selected-authority probe harness failed: {error}")
        }
        other => panic!("selected authority did not receive HTTP: {other:?}"),
    }
}

#[test]
fn backend_ca_unselected_authority_is_rejected_before_http() {
    assert_ne!(
        TEST_CA_SELECTED_EXCLUSIVE_PEM, TEST_CA_UNSELECTED_PEM,
        "selected and unselected fixture authorities must be independent"
    );
    let run = run_backend_ca_probe(TEST_SERVER_UNSELECTED_CA_PEM, TEST_SERVER_UNSELECTED_CA_KEY);
    assert_probe_threads_joined(&run);

    let error = run
        .client_result
        .as_ref()
        .expect("backend client result arrived")
        .as_ref()
        .expect_err("unselected server authority must be rejected");
    assert_eq!(
        error.chain.last().map(String::as_str),
        Some("invalid peer certificate: UnknownIssuer"),
        "client error chain did not end in the expected unknown-issuer verification cause: {:?}",
        error.chain
    );
    match run.observation.as_ref().expect("probe observation arrived") {
        ProbeObservation::RejectedBeforeHttp {
            alert: rustls::AlertDescription::UnknownCA,
        } => {}
        ProbeObservation::PartialHttp { bytes, error } => panic!(
            "unselected authority received {bytes} decrypted HTTP bytes before rejection: {error}"
        ),
        ProbeObservation::HarnessFailure(error) => {
            panic!("unselected-authority probe harness failed: {error}")
        }
        other => panic!("unselected authority had unexpected observation: {other:?}"),
    }
}

#[test]
fn backend_ca_wrong_host_is_rejected_before_http() {
    let run = run_backend_ca_probe(TEST_SERVER_WRONG_HOST_PEM, TEST_SERVER_WRONG_HOST_KEY);
    assert_probe_threads_joined(&run);

    let error = run
        .client_result
        .as_ref()
        .expect("backend client result arrived")
        .as_ref()
        .expect_err("wrong-host server certificate must be rejected");
    assert_eq!(
        error.chain.last().map(String::as_str),
        Some(
            "invalid peer certificate: certificate not valid for name \"127.0.0.1\"; certificate is only valid for DnsName(\"wrong-host.invalid\")"
        ),
        "client error chain did not end in the expected name/SAN verification cause: {:?}",
        error.chain
    );
    match run.observation.as_ref().expect("probe observation arrived") {
        ProbeObservation::RejectedBeforeHttp {
            alert: rustls::AlertDescription::BadCertificate,
        } => {}
        ProbeObservation::PartialHttp { bytes, error } => panic!(
            "wrong-host server received {bytes} decrypted HTTP bytes before rejection: {error}"
        ),
        ProbeObservation::HarnessFailure(error) => {
            panic!("wrong-host probe harness failed: {error}")
        }
        other => panic!("wrong-host server had unexpected observation: {other:?}"),
    }
}
