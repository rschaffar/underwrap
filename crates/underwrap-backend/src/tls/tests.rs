use super::{load_cert_chain, load_private_key};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use tempfile::NamedTempFile;

fn pem_block(label: &str, der: &[u8]) -> String {
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        STANDARD.encode(der)
    )
}

fn pem_file(contents: &str) -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    std::fs::write(file.path(), contents).unwrap();
    file
}

#[test]
fn certificate_loader_preserves_chain_order() {
    let file = pem_file(&format!(
        "{}{}{}",
        pem_block("PRIVATE KEY", &[9]),
        pem_block("CERTIFICATE", &[1, 2, 3]),
        pem_block("CERTIFICATE", &[4, 5]),
    ));

    let certs = load_cert_chain(file.path()).unwrap();
    let der: Vec<&[u8]> = certs.iter().map(AsRef::as_ref).collect();
    assert_eq!(der, vec![&[1, 2, 3][..], &[4, 5][..]]);
}

#[test]
fn certificate_loader_distinguishes_absence_from_malformed_pem() {
    let absent = pem_file(&pem_block("PRIVATE KEY", &[1]));
    let error = load_cert_chain(absent.path()).expect_err("missing certificate must fail");
    assert!(format!("{error:#}").contains("contained no certificates"));

    let malformed = pem_file("-----BEGIN CERTIFICATE-----\n%%%\n-----END CERTIFICATE-----\n");
    let error = load_cert_chain(malformed.path()).expect_err("malformed certificate must fail");
    assert!(format!("{error:#}").contains("parsing certificate PEM"));
}

#[test]
fn private_key_loader_accepts_each_supported_encoding() {
    for (label, der) in [
        ("RSA PRIVATE KEY", &[1, 2][..]),
        ("EC PRIVATE KEY", &[3, 4][..]),
        ("PRIVATE KEY", &[5, 6][..]),
    ] {
        let file = pem_file(&pem_block(label, der));
        let key = load_private_key(file.path()).unwrap();
        assert_eq!(key.secret_der(), der, "unexpected key bytes for {label}");
    }
}

#[test]
fn private_key_loader_selects_the_first_supported_key() {
    let file = pem_file(&format!(
        "{}{}{}",
        pem_block("CERTIFICATE", &[9]),
        pem_block("PRIVATE KEY", &[1, 2]),
        pem_block("RSA PRIVATE KEY", &[3, 4]),
    ));

    let key = load_private_key(file.path()).unwrap();
    assert_eq!(key.secret_der(), &[1, 2]);
}

#[test]
fn private_key_loader_distinguishes_absence_from_malformed_pem() {
    let absent = pem_file(&pem_block("CERTIFICATE", &[1]));
    let error = load_private_key(absent.path()).expect_err("missing private key must fail");
    assert!(format!("{error:#}").contains("contained no private key"));

    let malformed = pem_file("-----BEGIN PRIVATE KEY-----\n%%%\n-----END PRIVATE KEY-----\n");
    let error = load_private_key(malformed.path()).expect_err("malformed private key must fail");
    assert!(format!("{error:#}").contains("parsing private key PEM"));
}
