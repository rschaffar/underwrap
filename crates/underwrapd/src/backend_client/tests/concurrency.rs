use super::{
    TEST_CA_PEM, TEST_CLIENT_KEY, TEST_CLIENT_PEM, TEST_SERVER_KEY, TEST_SERVER_PEM, write_fixture,
};
use crate::backend_client::{BackendClient, MAX_IN_FLIGHT_REQUESTS, REQUEST_DEADLINE};
use rustls::pki_types::pem::PemObject;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use underwrap_protocol::config::BackendServiceConfig;
use underwrap_protocol::service::RuntimeWrapperInvocationOpenResponse;
use underwrap_protocol::variant::WrapperVariant;

const SOCKET_DEADLINE: Duration = Duration::from_secs(5);
const HELD_REQUEST_DEADLINE: Duration = Duration::from_secs(2);
const CLIENT_TEST_DEADLINE: Duration = Duration::from_secs(4);
const SHORT_REQUEST_DEADLINE: Duration = Duration::from_millis(500);
const MAX_TEST_HTTP_REQUEST_BYTES: usize = 16 * 1024;

type TlsStream = rustls::StreamOwned<rustls::ServerConnection, TcpStream>;
type RuntimeHandler =
    dyn Fn(usize, Vec<u8>, &mut TlsStream) -> Result<(), String> + Send + Sync + 'static;

#[test]
fn production_runtime_request_policy_is_pinned() {
    assert_eq!(MAX_IN_FLIGHT_REQUESTS, 32);
    assert_eq!(REQUEST_DEADLINE, Duration::from_secs(30));
}

fn server_config() -> Arc<rustls::ServerConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let certificate =
        rustls::pki_types::CertificateDer::from_pem_slice(TEST_SERVER_PEM.as_bytes()).unwrap();
    let private_key =
        rustls::pki_types::PrivateKeyDer::from_pem_slice(TEST_SERVER_KEY.as_bytes()).unwrap();
    Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate], private_key)
            .unwrap(),
    )
}

fn open_test_client(
    address: SocketAddr,
    max_in_flight: usize,
    request_deadline: Duration,
) -> (tempfile::TempDir, BackendClient) {
    let dir = tempfile::tempdir().unwrap();
    let ca = dir.path().join("ca.pem");
    let cert = dir.path().join("client.pem");
    let key = dir.path().join("client.key");
    write_fixture(&ca, TEST_CA_PEM);
    write_fixture(&cert, TEST_CLIENT_PEM);
    write_fixture(&key, TEST_CLIENT_KEY);
    let client = BackendClient::open_with_policy(
        &BackendServiceConfig {
            url: format!("https://127.0.0.1:{}", address.port()),
            ca_file: Some(ca),
            client_cert_file: Some(cert),
            client_key_file: Some(key),
        },
        max_in_flight,
        request_deadline,
    )
    .unwrap();
    (dir, client)
}

fn accept_before(listener: &TcpListener, deadline: Instant) -> Result<TcpStream, String> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(
                        "deadline elapsed before a runtime request reached the backend".to_string(),
                    );
                }
                std::thread::yield_now();
            }
            Err(error) => return Err(format!("accepting runtime request: {error}")),
        }
    }
}

fn read_complete_http_request(tls: &mut TlsStream) -> Result<Vec<u8>, String> {
    let mut request = Vec::new();
    let mut buffer = [0u8; 2048];

    loop {
        let read = tls
            .read(&mut buffer)
            .map_err(|error| format!("reading TLS HTTP request: {error}"))?;
        if read == 0 {
            return Err("connection closed before a complete HTTP request".to_string());
        }
        request.extend_from_slice(&buffer[..read]);
        if request.len() > MAX_TEST_HTTP_REQUEST_BYTES {
            return Err("HTTP request exceeded test harness limit".to_string());
        }

        let Some(header_offset) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
            continue;
        };
        let body_offset = header_offset + 4;
        let headers = std::str::from_utf8(&request[..header_offset])
            .map_err(|error| format!("HTTP headers were not UTF-8: {error}"))?;
        let content_length = headers
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map(|(_, value)| value.trim().parse::<usize>())
            .transpose()
            .map_err(|error| format!("invalid HTTP Content-Length: {error}"))?
            .unwrap_or(0);
        if request.len() >= body_offset + content_length {
            return Ok(request);
        }
    }
}

fn spawn_runtime_server(
    expected_connections: usize,
    handler: Arc<RuntimeHandler>,
) -> (SocketAddr, std::thread::JoinHandle<Result<(), String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server_config = server_config();
    let server = std::thread::spawn(move || {
        let mut workers = Vec::with_capacity(expected_connections);
        let mut accept_error = None;
        for index in 0..expected_connections {
            let stream = match accept_before(&listener, Instant::now() + SOCKET_DEADLINE) {
                Ok(stream) => stream,
                Err(error) => {
                    accept_error = Some(error);
                    break;
                }
            };
            stream
                .set_read_timeout(Some(SOCKET_DEADLINE))
                .map_err(|error| format!("setting runtime request read deadline: {error}"))?;
            stream
                .set_write_timeout(Some(SOCKET_DEADLINE))
                .map_err(|error| format!("setting runtime request write deadline: {error}"))?;
            let server_config = Arc::clone(&server_config);
            let handler = Arc::clone(&handler);
            workers.push(std::thread::spawn(move || {
                let connection = rustls::ServerConnection::new(server_config)
                    .map_err(|error| format!("creating TLS connection: {error}"))?;
                let mut tls = rustls::StreamOwned::new(connection, stream);
                let request = read_complete_http_request(&mut tls)?;
                handler(index, request, &mut tls)
            }));
        }
        drop(listener);

        let mut worker_error = None;
        for worker in workers {
            match worker.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) if worker_error.is_none() => worker_error = Some(error),
                Ok(Err(_)) => {}
                Err(_) if worker_error.is_none() => {
                    worker_error = Some("runtime server worker panicked".to_string())
                }
                Err(_) => {}
            }
        }
        if let Some(error) = accept_error.or(worker_error) {
            Err(error)
        } else {
            Ok(())
        }
    });
    (address, server)
}

fn expect_open_request(request: &[u8], wrapper_name: &str) -> Result<(), String> {
    let request = std::str::from_utf8(request)
        .map_err(|error| format!("runtime request was not UTF-8: {error}"))?;
    if !request.contains("POST /v1/wrapper-invocations HTTP/1.1")
        || !request.contains(&format!("\"wrapper_name\":\"{wrapper_name}\""))
    {
        return Err(format!(
            "runtime request did not target invocation-open for {wrapper_name}"
        ));
    }
    Ok(())
}

fn write_open_denial(tls: &mut TlsStream) -> Result<(), String> {
    let body = br#"{"outcome":"deny","reason":"locked"}"#;
    write!(
        tls,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .and_then(|()| tls.write_all(body))
    .and_then(|()| tls.flush())
    .map_err(|error| format!("writing runtime response: {error}"))
}

async fn receive_before_deadline(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<usize>,
) -> Option<usize> {
    tokio::time::timeout(SOCKET_DEADLINE, events.recv())
        .await
        .ok()
        .flatten()
}

#[tokio::test(flavor = "current_thread")]
async fn independent_runtime_requests_are_not_globally_serialized() {
    let (event_tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let handler = Arc::new(move |index, request: Vec<u8>, tls: &mut TlsStream| {
        let wrapper = if index == 0 {
            "slow-wrapper"
        } else {
            "fast-wrapper"
        };
        expect_open_request(&request, wrapper)?;
        event_tx
            .send(index)
            .map_err(|_| "reporting accepted runtime request".to_string())?;
        if index == 0 {
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(HELD_REQUEST_DEADLINE)
                .map_err(|error| format!("waiting to release first response: {error}"))?;
        }
        write_open_denial(tls)
    });
    let (address, server) = spawn_runtime_server(2, handler);
    let (_fixtures, client) = open_test_client(address, 2, CLIENT_TEST_DEADLINE);
    let client = Arc::new(client);

    let first_client = Arc::clone(&client);
    let first = tokio::spawn(async move {
        first_client
            .open_invocation("slow-wrapper", "slow-resource", WrapperVariant::Real)
            .await
    });
    let first_observation = receive_before_deadline(&mut events).await;
    let second_result = client
        .open_invocation("fast-wrapper", "fast-resource", WrapperVariant::Real)
        .await;
    let second_observation = events.try_recv().ok();

    let _ = release_tx.send(());
    let first_join = first.await;
    let server_result = server.join().expect("runtime server thread panicked");

    let first_result = first_join.expect("first client task panicked");
    assert_eq!(first_observation, Some(0));
    assert_eq!(second_observation, Some(1));
    assert!(matches!(
        second_result,
        Ok(RuntimeWrapperInvocationOpenResponse::Deny { .. })
    ));
    assert!(matches!(
        first_result,
        Ok(RuntimeWrapperInvocationOpenResponse::Deny { .. })
    ));
    server_result.expect("synchronized runtime server failed");
}

#[tokio::test(flavor = "current_thread")]
async fn accepted_request_timeout_releases_its_permit() {
    let (event_tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let handler = Arc::new(move |index, request: Vec<u8>, tls: &mut TlsStream| {
        let wrapper = if index == 0 {
            "timed-out-wrapper"
        } else {
            "later-wrapper"
        };
        expect_open_request(&request, wrapper)?;
        event_tx
            .send(index)
            .map_err(|_| "reporting accepted runtime request".to_string())?;
        if index == 0 {
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(HELD_REQUEST_DEADLINE)
                .map_err(|error| format!("waiting to close unanswered response: {error}"))?;
            Ok(())
        } else {
            write_open_denial(tls)
        }
    });
    let (address, server) = spawn_runtime_server(2, handler);
    let (_fixtures, client) = open_test_client(address, 1, SHORT_REQUEST_DEADLINE);
    let client = Arc::new(client);

    let first_client = Arc::clone(&client);
    let first = tokio::spawn(async move {
        first_client
            .open_invocation(
                "timed-out-wrapper",
                "timed-out-resource",
                WrapperVariant::Real,
            )
            .await
    });
    let first_observation = receive_before_deadline(&mut events).await;
    let first_join = first.await;
    let later_result = client
        .open_invocation("later-wrapper", "later-resource", WrapperVariant::Real)
        .await;
    let later_observation = receive_before_deadline(&mut events).await;

    let _ = release_tx.send(());
    let server_result = server.join().expect("runtime server thread panicked");

    let first_result = first_join.expect("timed-out client task panicked");
    assert_eq!(first_observation, Some(0));
    let timeout_error = first_result.expect_err("unanswered request must time out");
    let timeout_message = format!("{timeout_error:#}");
    assert!(timeout_message.contains("backend open request timed out"));
    assert!(!timeout_message.contains("timed-out-wrapper"));
    assert!(!timeout_message.contains("timed-out-resource"));
    assert_eq!(later_observation, Some(1));
    assert!(matches!(
        later_result,
        Ok(RuntimeWrapperInvocationOpenResponse::Deny { .. })
    ));
    server_result.expect("synchronized runtime server failed");
}

#[tokio::test(flavor = "current_thread")]
async fn saturation_is_fail_fast_without_starting_a_request_and_later_admission_succeeds() {
    let client = Arc::new(BackendClient {
        client: reqwest::Client::new(),
        base_url: "https://unused.invalid".to_string(),
        in_flight: tokio::sync::Semaphore::new(1),
        request_deadline: CLIENT_TEST_DEADLINE,
    });
    let (first_entered_tx, first_entered_rx) = tokio::sync::oneshot::channel();
    let (release_first_tx, release_first_rx) = tokio::sync::oneshot::channel();

    let first_client = Arc::clone(&client);
    let first = tokio::spawn(async move {
        first_client
            .execute("first", || async move {
                first_entered_tx
                    .send(())
                    .map_err(|()| anyhow::anyhow!("reporting first request entry"))?;
                release_first_rx
                    .await
                    .map_err(|_| anyhow::anyhow!("waiting to release first request"))?;
                Ok::<_, anyhow::Error>("first completed")
            })
            .await
    });
    tokio::time::timeout(HELD_REQUEST_DEADLINE, first_entered_rx)
        .await
        .expect("first request missed entry deadline")
        .expect("first request exited before reporting entry");

    let saturated_request_started = Arc::new(AtomicBool::new(false));
    let saturated_result = tokio::time::timeout(HELD_REQUEST_DEADLINE, {
        let saturated_request_started = Arc::clone(&saturated_request_started);
        client.execute("second", move || {
            saturated_request_started.store(true, Ordering::SeqCst);
            async { Ok::<_, anyhow::Error>("second completed") }
        })
    })
    .await
    .expect("saturated admission check missed deadline");

    let saturation_error = saturated_result.expect_err("second request must fail admission");
    assert!(
        format!("{saturation_error:#}")
            .contains("backend second request rejected: client is saturated")
    );
    assert!(
        !saturated_request_started.load(Ordering::SeqCst),
        "saturated admission must not construct or start the request future"
    );

    release_first_tx
        .send(())
        .expect("first request must still be waiting for release");
    let first_result = tokio::time::timeout(HELD_REQUEST_DEADLINE, first)
        .await
        .expect("first request missed completion deadline")
        .expect("first request task panicked")
        .expect("first admitted request failed");
    assert_eq!(first_result, "first completed");

    let later_request_started = Arc::new(AtomicBool::new(false));
    let later_result = tokio::time::timeout(HELD_REQUEST_DEADLINE, {
        let later_request_started = Arc::clone(&later_request_started);
        client.execute("later", move || {
            later_request_started.store(true, Ordering::SeqCst);
            async { Ok::<_, anyhow::Error>("later completed") }
        })
    })
    .await
    .expect("later admission missed deadline")
    .expect("later request must be admitted");
    assert_eq!(later_result, "later completed");
    assert!(
        later_request_started.load(Ordering::SeqCst),
        "later admitted request must start"
    );
}
