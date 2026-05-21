use super::*;

fn sample_cert_pem(label: &str) -> String {
    let key = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::new(vec![label.to_string()]).unwrap();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, label);
    params.not_before = rcgen::date_time_ymd(2025, 1, 1);
    params.not_after = rcgen::date_time_ymd(2035, 1, 1);
    params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    params.serial_number = Some(rcgen::SerialNumber::from(1));
    let cert = params.self_signed(&key).unwrap();
    cert.pem()
}

fn sample(id: &str) -> ClientInstanceRecord {
    ClientInstanceRecord {
        id: id.to_string(),
        display_name: format!("Display {id}"),
        current_store_id: "store_work".into(),
        status: ClientInstanceStatus::Active,
        last_seen_at: Some(123),
        current_credential: current_credential_from_pem(sample_cert_pem(id), Some(100)).unwrap(),
        auto_create_wrapper_assets: false,
    }
}

#[test]
fn missing_registry_file_is_tolerated() {
    let tmp = tempfile::TempDir::new().unwrap();
    let loaded = load_client_instances(&tmp.path().join("client-instances.json")).unwrap();
    assert!(loaded.is_empty());
}

#[test]
fn save_and_load_round_trip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("client-instances.json");
    let mut map = HashMap::new();
    map.insert("inst_work".into(), sample("inst_work"));
    save_client_instances(&path, &map).unwrap();
    let loaded = load_client_instances(&path).unwrap();
    assert_eq!(loaded, map);
}

#[test]
fn invalid_version_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("client-instances.json");
    std::fs::write(&path, r#"{"version":99,"instances":[]}"#).unwrap();
    let err = load_client_instances(&path).expect_err("unsupported version must fail");
    assert!(format!("{err:#}").contains("unsupported runtime client registry version"));
}

#[test]
fn duplicate_id_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("client-instances.json");
    let a = sample("inst_a");
    let mut b = sample("inst_b");
    b.id = a.id.clone();
    let doc = ClientInstancesDocument {
        version: 1,
        instances: vec![a, b],
    };
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let err = load_client_instances(&path).expect_err("duplicate ids must fail");
    assert!(format!("{err:#}").contains("duplicate runtime client instance id"));
}

#[test]
fn duplicate_fingerprint_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("client-instances.json");
    let a = sample("inst_a");
    let mut b = sample("inst_b");
    b.current_credential.cert_fingerprint = a.current_credential.cert_fingerprint.clone();
    b.current_credential.cert_pem = a.current_credential.cert_pem.clone();
    let doc = ClientInstancesDocument {
        version: 1,
        instances: vec![a, b],
    };
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let err = load_client_instances(&path).expect_err("duplicate fingerprints must fail");
    assert!(format!("{err:#}").contains("duplicate runtime client credential fingerprint"));
}

#[test]
fn mismatched_current_credential_fingerprint_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("client-instances.json");
    let mut rec = sample("inst_a");
    rec.current_credential.cert_fingerprint = "wrong".into();
    let doc = ClientInstancesDocument {
        version: 1,
        instances: vec![rec],
    };
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let err = load_client_instances(&path).expect_err("mismatched fingerprints must fail");
    assert!(format!("{err:#}").contains("fingerprint/PEM mismatch"));
}

#[test]
fn multiple_certificates_in_current_credential_are_rejected() {
    let err = cert_fingerprint_from_pem(&format!(
        "{}{}",
        sample_cert_pem("first"),
        sample_cert_pem("second")
    ))
    .expect_err("multiple certs must be rejected");
    assert!(format!("{err:#}").contains("exactly one certificate"));
}

#[test]
fn empty_required_fields_are_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("client-instances.json");
    let mut rec = sample("inst_a");
    rec.display_name.clear();
    let doc = ClientInstancesDocument {
        version: 1,
        instances: vec![rec],
    };
    std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
    let err = load_client_instances(&path).expect_err("empty required fields must fail");
    assert!(format!("{err:#}").contains("empty display name"));
}
