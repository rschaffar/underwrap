use super::*;

#[test]
fn yaml_and_generated_json_round_trip_with_defaults_and_optional_nulls() {
    let config = parse_backend_config(
        "stores_dir: /var/lib/秘密\nhttps: null\nadmin: {allowed_origins: ['null', '~', 'https://例.example']}\n",
    )
    .unwrap();
    assert_eq!(config.stores_dir, PathBuf::from("/var/lib/秘密"));
    assert!(config.https.is_none());
    let admin = config.admin.as_ref().unwrap();
    assert_eq!(admin.allowed_origins, ["null", "~", "https://例.example"]);
    assert_eq!(
        admin.session_lifetime_secs,
        crate::DEFAULT_SESSION_LIFETIME_SECS
    );
    assert!(admin.stores_data_root.is_none());
    let expected = serde_json::to_value(&config).unwrap();
    for text in [
        serde_saphyr::to_string(&config).unwrap(),
        serde_json::to_string(&config).unwrap(),
    ] {
        assert_eq!(
            serde_json::to_value(parse_backend_config(&text).unwrap()).unwrap(),
            expected
        );
    }
}

#[test]
fn shipped_full_example_round_trips_through_production_parser() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/backend.yaml");
    let config = parse_backend_config(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(config.https.is_some());
    assert!(config.admin.is_some());
    let restored = parse_backend_config(&serde_saphyr::to_string(&config).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(config).unwrap(),
        serde_json::to_value(restored).unwrap()
    );
}

#[test]
fn rejects_duplicates_malformed_types_required_fields_and_multiple_documents() {
    for text in [
        "stores_dir: /one\nstores_dir: /two",
        "admin: {session_lifetime_secs: 1, session_lifetime_secs: 2}",
        "ignored: {nested: {same: a, same: b}}",
        "stores_dir: []",
        "https: {}",
        "https: {listen: '127.0.0.1:8443', tls: {cert_file: /cert}}",
        "admin: {session_lifetime_secs: wrong}",
        "{}\n---\n{}",
    ] {
        assert!(parse_backend_config(text).is_err(), "accepted {text:?}");
    }
}

#[test]
fn ordinary_aliases_work_but_merge_fields_remain_unexpanded() {
    let config = parse_backend_config("stores_dir: &path /one\nclients_dir: *path").unwrap();
    assert_eq!(config.stores_dir, config.clients_dir);
    // These existing structs ignore unknown fields; a literal merge must not
    // become an override or supply missing required fields in a nested struct.
    let config = parse_backend_config("<<: {stores_dir: /override}").unwrap();
    assert_eq!(
        config.stores_dir,
        PathBuf::from(crate::config::DEFAULT_STORES_DIR)
    );
    assert!(
        parse_backend_config(
            "https: {<<: {listen: '127.0.0.1:8443', tls: {cert_file: /cert, key_file: /key}}}"
        )
        .is_err()
    );
    assert!(parse_backend_config("ignored: &cycle [*cycle]").is_err());
}
