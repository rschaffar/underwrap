use super::*;

#[test]
fn distinct_output_paths_are_required() {
    let path = Path::new("/tmp/same.pem");
    let err = validate_output_paths(path, path).unwrap_err();
    assert!(format!("{err:#}").contains("must be distinct"));
}

#[test]
fn key_and_self_signed_cert_are_generated_locally() {
    let (key_pem, cert_pem) = generate_key_and_self_signed_cert().unwrap();
    assert!(key_pem.contains("BEGIN PRIVATE KEY"));
    assert!(cert_pem.contains("BEGIN CERTIFICATE"));
    assert!(!cert_pem.contains("CERTIFICATE REQUEST"));
}

fn explicit_cli(cert: &str, key: &str) -> Cli {
    Cli {
        client_cert_out: Some(PathBuf::from(cert)),
        client_key_out: Some(PathBuf::from(key)),
        instance_config: None,
    }
}

#[test]
fn resolve_targets_uses_explicit_flags() {
    let cli = explicit_cli("/tmp/c.pem", "/tmp/c.key");
    let t = cli.resolve_targets().unwrap();
    assert_eq!(t.client_cert_out, PathBuf::from("/tmp/c.pem"));
    assert_eq!(t.client_key_out, PathBuf::from("/tmp/c.key"));
}

#[test]
fn resolve_targets_reads_instance_config() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = tmp.path().join("work.yaml");
    std::fs::write(
        &cfg,
        r#"
backend:
  url: https://backend.example.internal:8443
  client_cert_file: /etc/underwrap/client-work.pem
  client_key_file: /etc/underwrap/client-work.key
wrappers: {}
"#,
    )
    .unwrap();
    let cli = Cli {
        client_cert_out: None,
        client_key_out: None,
        instance_config: Some(cfg),
    };
    let t = cli.resolve_targets().unwrap();
    assert_eq!(
        t.client_cert_out,
        PathBuf::from("/etc/underwrap/client-work.pem")
    );
    assert_eq!(
        t.client_key_out,
        PathBuf::from("/etc/underwrap/client-work.key")
    );
}

#[test]
fn resolve_targets_rejects_instance_config_without_cert_paths() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = tmp.path().join("noop.yaml");
    std::fs::write(
        &cfg,
        r#"
backend:
  url: https://backend.example.internal:8443
wrappers: {}
"#,
    )
    .unwrap();
    let cli = Cli {
        client_cert_out: None,
        client_key_out: None,
        instance_config: Some(cfg),
    };
    let err = cli.resolve_targets().unwrap_err();
    assert!(format!("{err:#}").contains("client_cert_file"));
}

#[test]
fn resolve_targets_requires_paths_without_instance_config() {
    let cli = Cli {
        client_cert_out: None,
        client_key_out: Some(PathBuf::from("/tmp/c.key")),
        instance_config: None,
    };
    let err = cli.resolve_targets().unwrap_err();
    assert!(format!("{err:#}").contains("--client-cert-out"));
}

#[cfg(unix)]
#[test]
fn install_credential_writes_expected_permissions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let key = tmp.path().join("client.key");
    let cert = tmp.path().join("client.pem");
    install_credential(&key, b"KEY", 0o400, &cert, b"CERT", 0o440, None).unwrap();
    assert_eq!(std::fs::read(&key).unwrap(), b"KEY");
    assert_eq!(std::fs::read(&cert).unwrap(), b"CERT");
    assert_eq!(
        std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
        0o400
    );
    assert_eq!(
        std::fs::metadata(&cert).unwrap().permissions().mode() & 0o777,
        0o440
    );
}

#[test]
fn derive_instance_owner_returns_none_for_unknown_user() {
    assert!(derive_instance_owner("definitely-not-an-existing-instance-xyzzy").is_none());
}

#[test]
fn derive_instance_owner_returns_none_for_empty_instance() {
    assert!(derive_instance_owner("").is_none());
    assert!(derive_instance_owner("   ").is_none());
}

#[cfg(unix)]
#[test]
fn resolve_target_owner_prefers_explicit_then_existing_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("client.pem");

    // No file, no preference → None
    assert!(resolve_target_owner(&path, None).is_none());

    // File exists → preserves its uid/gid
    std::fs::write(&path, b"x").unwrap();
    let meta = std::fs::metadata(&path).unwrap();
    use std::os::unix::fs::MetadataExt;
    let (uid, gid) = (meta.uid(), meta.gid());
    assert_eq!(resolve_target_owner(&path, None), Some((uid, gid)));

    // Explicit preference wins over existing file
    assert_eq!(
        resolve_target_owner(&path, Some((12345, 67890))),
        Some((12345, 67890))
    );
}
