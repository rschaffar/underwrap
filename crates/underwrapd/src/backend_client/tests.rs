use super::*;
use std::io::Write;

#[test]
fn open_fails_immediately_on_bad_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let ca = dir.path().join("ca.pem");
    let cert = dir.path().join("cert.pem");
    let key = dir.path().join("key.pem");
    for path in [&ca, &cert, &key] {
        std::fs::File::create(path)
            .unwrap()
            .write_all(b"not-a-cert")
            .unwrap();
    }
    let result = BackendClient::open(&BackendServiceConfig {
        url: "https://localhost:0".to_string(),
        ca_file: Some(ca),
        client_cert_file: Some(cert),
        client_key_file: Some(key),
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
        client_cert_file: None,
        client_key_file: None,
    });
    assert!(
        format!("{:#}", result.err().unwrap()).contains("client_cert_file"),
        "expected client_cert_file error",
    );
}
