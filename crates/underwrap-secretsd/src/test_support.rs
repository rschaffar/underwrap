use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::RwLock;
use zbus::{Connection, Guid, connection::Builder};

use underwrap_protocol::libsecret_publication::{
    REQUEST_HEADER_LEN, ResponseKind, decode_request_header, encode_response,
};
use underwrap_protocol::resource_blob::LibsecretSection;
use underwrap_secretsd_store::SecretStore;

use crate::dbus_service::{CollectionInterface, ItemInterface, SecretService};
use crate::publication::{Publisher, SharedPublisher};

const TEST_IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
pub(crate) enum HostResponse {
    Ack,
    Reject,
    WrongGeneration,
    Eof,
}

#[derive(Clone, Debug)]
pub(crate) struct ObservedPublication {
    pub(crate) generation: u64,
    pub(crate) section: LibsecretSection,
}

pub(crate) struct ScriptedPublisher {
    publisher: SharedPublisher,
    observed: Arc<Mutex<Vec<ObservedPublication>>>,
    shutdown: Option<UnixStream>,
    responder: Option<JoinHandle<Result<usize, String>>>,
    expected: usize,
}

impl ScriptedPublisher {
    pub(crate) fn new(script: Vec<HostResponse>) -> Self {
        let expected = script.len();
        let (publisher_stream, mut host) = UnixStream::pair().unwrap();
        host.set_read_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
        host.set_write_timeout(Some(TEST_IO_TIMEOUT)).unwrap();
        let shutdown = host.try_clone().unwrap();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let responder_observed = observed.clone();
        let responder = std::thread::spawn(move || {
            let mut processed = 0;
            for response in script {
                let mut header = [0; REQUEST_HEADER_LEN];
                host.read_exact(&mut header)
                    .map_err(|error| format!("reading publication header: {error}"))?;
                let header = decode_request_header(&header)
                    .map_err(|error| format!("decoding publication header: {error}"))?;
                let mut body = vec![0; header.body_len];
                host.read_exact(&mut body)
                    .map_err(|error| format!("reading publication body: {error}"))?;
                let section = LibsecretSection::from_strict_publication_bytes(&body)
                    .map_err(|error| format!("decoding publication body: {error:#}"))?;
                responder_observed
                    .lock()
                    .map_err(|_| "publication observation lock poisoned".to_string())?
                    .push(ObservedPublication {
                        generation: header.generation,
                        section,
                    });
                processed += 1;

                match response {
                    HostResponse::Ack => host
                        .write_all(&encode_response(ResponseKind::Ack, header.generation))
                        .map_err(|error| format!("writing ACK: {error}"))?,
                    HostResponse::Reject => host
                        .write_all(&encode_response(ResponseKind::Reject, header.generation))
                        .map_err(|error| format!("writing REJECT: {error}"))?,
                    HostResponse::WrongGeneration => host
                        .write_all(&encode_response(
                            ResponseKind::Ack,
                            header.generation.wrapping_add(1),
                        ))
                        .map_err(|error| format!("writing wrong-generation ACK: {error}"))?,
                    HostResponse::Eof => {
                        host.shutdown(Shutdown::Both)
                            .map_err(|error| format!("closing publication host: {error}"))?;
                        return Ok(processed);
                    }
                }
            }
            host.shutdown(Shutdown::Both)
                .map_err(|error| format!("closing publication host: {error}"))?;
            Ok(processed)
        });
        let publisher = Arc::new(Mutex::new(Publisher::from_stream(publisher_stream)));
        Self {
            publisher,
            observed,
            shutdown: Some(shutdown),
            responder: Some(responder),
            expected,
        }
    }

    pub(crate) fn publisher(&self) -> SharedPublisher {
        self.publisher.clone()
    }

    pub(crate) fn finish(mut self) -> Vec<ObservedPublication> {
        let result = self.stop();
        assert_eq!(result.as_ref().map(|count| *count), Ok(self.expected));
        self.observed.lock().unwrap().clone()
    }

    fn stop(&mut self) -> Result<usize, String> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.shutdown(Shutdown::Both);
        }
        self.responder
            .take()
            .expect("scripted publisher responder remains joinable")
            .join()
            .map_err(|_| "scripted publisher responder panicked".to_string())?
    }
}

impl Drop for ScriptedPublisher {
    fn drop(&mut self) {
        if self.responder.is_some() {
            let _ = self.stop();
        }
    }
}

pub(crate) fn service_with_script(
    store: SecretStore,
    script: Vec<HostResponse>,
) -> (SecretService, Arc<RwLock<SecretStore>>, ScriptedPublisher) {
    let script = ScriptedPublisher::new(script);
    let store = Arc::new(RwLock::new(store));
    let service = SecretService::new(store.clone(), script.publisher());
    (service, store, script)
}

pub(crate) struct ObjectServerHarness {
    server: Option<Connection>,
    shutdown: Vec<UnixStream>,
}

impl ObjectServerHarness {
    pub(crate) async fn new() -> Self {
        let (server_stream, peer_stream) = UnixStream::pair().unwrap();
        server_stream.set_nonblocking(true).unwrap();
        peer_stream.set_nonblocking(true).unwrap();
        let shutdown = vec![
            server_stream.try_clone().unwrap(),
            peer_stream.try_clone().unwrap(),
        ];
        let server_stream = tokio::net::UnixStream::from_std(server_stream).unwrap();
        let server = Builder::authenticated_socket(server_stream, Guid::generate())
            .unwrap()
            .build()
            .await
            .unwrap();
        Self {
            server: Some(server),
            shutdown,
        }
    }

    pub(crate) fn object_server(&self) -> impl std::ops::Deref<Target = zbus::ObjectServer> + '_ {
        self.server
            .as_ref()
            .expect("object-server connection remains open")
            .object_server()
    }

    pub(crate) async fn has_collection(&self, path: &str) -> bool {
        self.object_server()
            .interface::<_, CollectionInterface>(path)
            .await
            .is_ok()
    }

    pub(crate) async fn has_item(&self, path: &str) -> bool {
        self.object_server()
            .interface::<_, ItemInterface>(path)
            .await
            .is_ok()
    }

    pub(crate) async fn close(mut self) {
        for stream in self.shutdown.drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
        let server = self.server.take().unwrap();
        let _ = server.close().await;
    }
}

/// Real D-Bus messages over a peer connection by default. Set
/// UNDERWRAP_TEST_DBUS_DAEMON to also run the same witnesses on a private bus.
/// No brokered values are written to the fixture directory.
pub(crate) struct WireHarness {
    pub(crate) server: Connection,
    pub(crate) client: Connection,
    bus: Option<TestBus>,
}

struct TestBus {
    child: std::process::Child,
    _directory: tempfile::TempDir,
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl WireHarness {
    pub(crate) async fn new(store: &Arc<RwLock<SecretStore>>, publisher: &SharedPublisher) -> Self {
        let (server, client, bus) =
            if let Some(executable) = std::env::var_os("UNDERWRAP_TEST_DBUS_DAEMON") {
                use std::io::BufRead;
                let directory = tempfile::tempdir().unwrap();
                let child = std::process::Command::new(executable)
                    .args(["--session", "--nofork", "--print-address=1"])
                    .arg(format!(
                        "--address=unix:path={}/bus",
                        directory.path().display()
                    ))
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .expect("start requested private D-Bus daemon");
                // Own/reap the child before readiness or connection can fail.
                let mut bus = TestBus {
                    child,
                    _directory: directory,
                };
                let mut address = String::new();
                std::io::BufReader::new(bus.child.stdout.take().unwrap())
                    .read_line(&mut address)
                    .unwrap();
                assert!(
                    !address.trim().is_empty(),
                    "private bus must report readiness"
                );
                let server = Builder::address(address.trim())
                    .unwrap()
                    .serve_at(
                        crate::BASE_PATH,
                        SecretService::new(store.clone(), publisher.clone()),
                    )
                    .unwrap()
                    .build()
                    .await
                    .unwrap();
                let client = Builder::address(address.trim())
                    .unwrap()
                    .build()
                    .await
                    .unwrap();
                server.request_name(crate::WELL_KNOWN_NAME).await.unwrap();
                (server, client, Some(bus))
            } else {
                let (server, client) = tokio::net::UnixStream::pair().unwrap();
                let guid = Guid::generate();
                let (server, client) = tokio::join!(
                    Builder::authenticated_socket(server, guid.clone())
                        .unwrap()
                        .serve_at(
                            crate::BASE_PATH,
                            SecretService::new(store.clone(), publisher.clone())
                        )
                        .unwrap()
                        .build(),
                    Builder::authenticated_socket(client, guid).unwrap().build(),
                );
                (server.unwrap(), client.unwrap(), None)
            };
        crate::dbus_service::register_objects(&server.object_server(), store, publisher)
            .await
            .unwrap();

        Self {
            server,
            client,
            bus,
        }
    }

    pub(crate) async fn proxy<'a>(&'a self, path: &'a str, interface: &'a str) -> zbus::Proxy<'a> {
        zbus::proxy::Builder::new(&self.client)
            .destination(
                self.server
                    .unique_name()
                    .map(|name| name.as_str())
                    .unwrap_or(":1.0"),
            )
            .unwrap()
            .path(path)
            .unwrap()
            .interface(interface)
            .unwrap()
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await
            .unwrap()
    }

    pub(crate) async fn close(self) {
        let _ = self.client.close().await;
        let _ = self.server.close().await;
        drop(self.bus);
    }
}

impl Drop for ObjectServerHarness {
    fn drop(&mut self) {
        for stream in self.shutdown.drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}
