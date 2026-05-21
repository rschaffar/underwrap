use super::*;
use crate::test_support::service_state_with_runtime_client;

#[test]
fn resolve_runtime_client_maps_cert_to_active_instance_store() {
    let cert_der = vec![1u8, 2, 3, 4];
    let state = service_state_with_runtime_client(
        &cert_der,
        crate::runtime_clients::ClientInstanceStatus::Active,
        "work",
    );
    let cert = rustls::pki_types::CertificateDer::from(cert_der);
    let resolved = resolve_runtime_client(&state, &[cert]).unwrap();
    assert_eq!(resolved.instance_id, "inst_local");
    assert_eq!(resolved.store_id, "work");
}

#[test]
fn resolve_runtime_client_rejects_disabled_instance() {
    let cert_der = vec![9u8, 8, 7];
    let state = service_state_with_runtime_client(
        &cert_der,
        crate::runtime_clients::ClientInstanceStatus::Disabled,
        "work",
    );
    let cert = rustls::pki_types::CertificateDer::from(cert_der);
    let err = match resolve_runtime_client(&state, &[cert]) {
        Ok(_) => panic!("disabled instances must fail"),
        Err(err) => err,
    };
    assert!(format!("{err:#}").contains("not active"));
}

#[test]
fn resolve_runtime_client_rejects_unknown_fingerprint() {
    let state = service_state_with_runtime_client(
        &[1u8, 2, 3],
        crate::runtime_clients::ClientInstanceStatus::Active,
        "work",
    );
    let cert = rustls::pki_types::CertificateDer::from(vec![4u8, 5, 6]);
    let err = match resolve_runtime_client(&state, &[cert]) {
        Ok(_) => panic!("unknown fingerprints must fail"),
        Err(err) => err,
    };
    assert!(format!("{err:#}").contains("unknown client credential fingerprint"));
}
