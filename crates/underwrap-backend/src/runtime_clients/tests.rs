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
        access_override: Default::default(),
        id: id.to_string(),
        display_name: format!("Display {id}"),
        current_store_id: "store_work".into(),
        status: ClientInstanceStatus::Active,
        last_seen_at: Some(123),
        current_credential: current_credential_from_pem(sample_cert_pem(id)).unwrap(),
        auto_create_wrapper_assets: false,
    }
}

#[test]
fn omitted_override_defaults_to_individual_and_explicit_modes_survive_reload() {
    use underwrap_protocol::policy::ClientAccessOverride;

    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("clients");
    let mut record = sample("inst_override");
    let mut value = serde_json::to_value(&record).unwrap();
    value.as_object_mut().unwrap().remove("access_override");
    let parsed: ClientInstanceRecord = serde_json::from_value(value).unwrap();
    assert_eq!(parsed.access_override, ClientAccessOverride::Individual {});

    for access in [
        ClientAccessOverride::Locked {},
        ClientAccessOverride::Unlocked { writable: false },
        ClientAccessOverride::Unlocked { writable: true },
        ClientAccessOverride::Individual {},
    ] {
        record.access_override = access;
        save_client_instance(&dir, &record).unwrap();
        let loaded = load_client_instances(&dir).unwrap();
        assert_eq!(loaded[&record.id].access_override, access);
    }
}

#[test]
fn missing_clients_dir_is_tolerated() {
    let tmp = tempfile::TempDir::new().unwrap();
    let loaded = load_client_instances(&tmp.path().join("clients")).unwrap();
    assert!(loaded.is_empty());
}

#[test]
fn save_and_load_round_trip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("clients");
    let record = sample("inst_work");
    save_client_instance(&dir, &record).unwrap();
    let loaded = load_client_instances(&dir).unwrap();
    let mut expected = HashMap::new();
    expected.insert("inst_work".into(), record);
    assert_eq!(loaded, expected);
}

#[test]
fn id_lives_in_filename_not_body() {
    // The on-disk JSON must not carry the id — the file stem is the
    // sole source of truth. Operators are free to rename a record by
    // renaming its file; rewriting the body would not move the id.
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("clients");
    let record = sample("inst_work");
    save_client_instance(&dir, &record).unwrap();
    let text = std::fs::read_to_string(dir.join("inst_work.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    let obj = parsed.as_object().unwrap();
    assert!(
        !obj.contains_key("id"),
        "on-disk client instance JSON must not contain an 'id' key: {obj:?}",
    );
    // And it must not contain the dropped `issued_at` either.
    let cred = obj["current_credential"].as_object().unwrap();
    assert!(
        !cred.contains_key("issued_at"),
        "on-disk current_credential must not contain 'issued_at': {cred:?}",
    );
}

#[test]
fn delete_removes_file_and_is_idempotent() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("clients");
    let record = sample("inst_gone");
    save_client_instance(&dir, &record).unwrap();
    assert!(dir.join("inst_gone.json").exists());
    delete_client_instance(&dir, "inst_gone").unwrap();
    assert!(!dir.join("inst_gone.json").exists());
    // Second delete is a no-op (idempotent under retry).
    delete_client_instance(&dir, "inst_gone").unwrap();
}

#[test]
fn duplicate_fingerprint_across_files_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("clients");
    let a = sample("inst_a");
    let mut b = sample("inst_b");
    b.current_credential.cert_fingerprint = a.current_credential.cert_fingerprint.clone();
    b.current_credential.cert_pem = a.current_credential.cert_pem.clone();
    save_client_instance(&dir, &a).unwrap();
    save_client_instance(&dir, &b).unwrap();
    let err = load_client_instances(&dir).expect_err("duplicate fingerprints must fail");
    assert!(
        format!("{err:#}").contains("duplicate runtime client instance credential fingerprint")
    );
}

#[test]
fn duplicate_display_name_across_files_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("clients");
    let a = sample("inst_a");
    let mut b = sample("inst_b");
    b.display_name = a.display_name.clone();
    save_client_instance(&dir, &a).unwrap();
    save_client_instance(&dir, &b).unwrap();
    let err = load_client_instances(&dir).expect_err("duplicate display names must fail");
    assert!(format!("{err:#}").contains("duplicate runtime client instance display name"));
}

#[test]
fn mismatched_current_credential_fingerprint_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("clients");
    std::fs::create_dir_all(&dir).unwrap();
    let mut rec = sample("inst_a");
    rec.current_credential.cert_fingerprint = "wrong".into();
    // Serialise by hand to skip the `save_client_instance` invariant
    // check on the PEM; we want the loader to be the one to catch it.
    let json = serde_json::to_vec_pretty(&rec).unwrap();
    std::fs::write(dir.join("inst_a.json"), &json).unwrap();
    let err = load_client_instances(&dir).expect_err("mismatched fingerprints must fail");
    assert!(format!("{err:#}").contains("fingerprint/PEM mismatch"));
}

#[test]
fn missing_certificate_in_current_credential_is_rejected() {
    let err = cert_fingerprint_from_pem("").expect_err("missing certificate must be rejected");
    assert!(format!("{err:#}").contains("contained no certificate"));
}

#[test]
fn malformed_certificate_in_current_credential_is_rejected() {
    let err =
        cert_fingerprint_from_pem("-----BEGIN CERTIFICATE-----\n%%%\n-----END CERTIFICATE-----\n")
            .expect_err("malformed certificate must be rejected");
    assert!(format!("{err:#}").contains("parsing client certificate PEM"));
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
    let dir = tmp.path().join("clients");
    std::fs::create_dir_all(&dir).unwrap();
    let mut rec = sample("inst_a");
    rec.display_name.clear();
    let json = serde_json::to_vec_pretty(&rec).unwrap();
    std::fs::write(dir.join("inst_a.json"), &json).unwrap();
    let err = load_client_instances(&dir).expect_err("empty required fields must fail");
    assert!(format!("{err:#}").contains("empty display name"));
}

#[test]
fn invalid_filename_stem_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("clients");
    std::fs::create_dir_all(&dir).unwrap();
    let rec = sample("placeholder");
    // The stem here contains a slash-equivalent character that is not
    // valid for an id; the loader must refuse it rather than silently
    // accept a malformed key.
    let json = serde_json::to_vec_pretty(&rec).unwrap();
    std::fs::write(dir.join("not a valid id.json"), &json).unwrap();
    let err = load_client_instances(&dir).expect_err("invalid filename stems must fail");
    assert!(format!("{err:#}").contains("invalid runtime client instance id"));
}

#[test]
fn non_json_files_are_ignored() {
    // Operators may keep `<id>.json.bak` backups in the directory;
    // those must not turn into loaded records.
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("clients");
    let record = sample("inst_work");
    save_client_instance(&dir, &record).unwrap();
    std::fs::write(dir.join("inst_work.json.bak"), b"junk").unwrap();
    std::fs::write(dir.join("README"), b"notes").unwrap();
    let loaded = load_client_instances(&dir).unwrap();
    assert_eq!(loaded.len(), 1);
    assert!(loaded.contains_key("inst_work"));
}
