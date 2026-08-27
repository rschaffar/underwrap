use super::*;
use clap::Parser;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn explicit_cli(identity: &str) -> Cli {
    Cli {
        client_identity_out: Some(PathBuf::from(identity)),
        instance_config: None,
    }
}

fn instance_config_cli(identity: &str) -> (tempfile::TempDir, Cli) {
    let tmp = tempfile::TempDir::new().unwrap();
    let config = tmp.path().join("work.yaml");
    let backend = BTreeMap::from([
        ("url", "https://backend.example.internal:8443".to_string()),
        ("client_identity_file", identity.to_string()),
    ]);
    let document = BTreeMap::from([("backend", backend)]);
    std::fs::write(&config, serde_yaml::to_string(&document).unwrap()).unwrap();
    let cli = Cli {
        client_identity_out: None,
        instance_config: Some(config),
    };
    (tmp, cli)
}

fn validate_cli_output(cli: &Cli) -> Result<()> {
    let targets = cli.resolve_targets()?;
    validate_output_path(&targets.client_identity_out)
}

#[test]
fn key_and_self_signed_cert_are_generated_locally_in_identity_order() {
    let (identity_pem, cert_pem) = generate_key_and_self_signed_cert().unwrap();
    let identity = std::str::from_utf8(identity_pem.as_slice()).unwrap();
    assert!(cert_pem.contains("BEGIN CERTIFICATE"));
    assert!(!cert_pem.contains("CERTIFICATE REQUEST"));
    assert!(identity.starts_with(&cert_pem));
    assert!(identity.contains("BEGIN PRIVATE KEY"));
    assert!(
        identity.find("BEGIN CERTIFICATE").unwrap() < identity.find("BEGIN PRIVATE KEY").unwrap()
    );
}

#[test]
fn resolve_targets_uses_explicit_flag() {
    let cli = explicit_cli("/tmp/client.identity.pem");
    let targets = cli.resolve_targets().unwrap();
    assert_eq!(
        targets.client_identity_out,
        PathBuf::from("/tmp/client.identity.pem")
    );
}

#[test]
fn explicit_output_conflicts_with_instance_config() {
    let error = Cli::try_parse_from([
        "underwrap-keygen",
        "--client-identity-out",
        "/tmp/client.identity.pem",
        "--instance-config",
        "/etc/underwrap/daemon/work.yaml",
    ])
    .unwrap_err();
    assert!(error.to_string().contains("cannot be used with"));
}

#[test]
fn resolve_targets_reads_instance_config() {
    let tmp = tempfile::TempDir::new().unwrap();
    let config = tmp.path().join("work.yaml");
    std::fs::write(
        &config,
        r#"
backend:
  url: https://backend.example.internal:8443
  client_identity_file: /var/lib/underwrap-work/client-work.identity.pem
wrappers: {}
"#,
    )
    .unwrap();
    let cli = Cli {
        client_identity_out: None,
        instance_config: Some(config),
    };
    let targets = cli.resolve_targets().unwrap();
    assert_eq!(
        targets.client_identity_out,
        PathBuf::from("/var/lib/underwrap-work/client-work.identity.pem")
    );
}

#[test]
fn output_path_gate_accepts_canonical_paths_in_explicit_and_config_modes() {
    for path in [
        "/",
        "/var/lib/underwrap/client.identity.pem",
        "/var/lib/秘密/身份.identity.pem",
    ] {
        validate_cli_output(&explicit_cli(path)).unwrap();

        let (_tmp, cli) = instance_config_cli(path);
        validate_cli_output(&cli).unwrap();
    }
}

#[test]
fn output_path_gate_rejects_noncanonical_paths_in_explicit_and_config_modes() {
    for path in [
        "",
        "relative/client.identity.pem",
        "/var/lib/./client.identity.pem",
        "/var/lib/../client.identity.pem",
        "/var//lib/client.identity.pem",
        "/var/lib/client.identity.pem/",
        "/var/lib/client identity.pem",
        "/var/lib/client\u{7f}identity.pem",
    ] {
        let error = validate_cli_output(&explicit_cli(path)).unwrap_err();
        assert!(
            format!("{error:#}").contains("canonical absolute path"),
            "explicit path {path:?} produced unexpected error: {error:#}"
        );

        let (_tmp, cli) = instance_config_cli(path);
        let error = validate_cli_output(&cli).unwrap_err();
        assert!(
            format!("{error:#}").contains("canonical absolute path"),
            "config-derived path {path:?} produced unexpected error: {error:#}"
        );
    }
}

#[test]
fn resolve_targets_rejects_instance_config_without_identity_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let config = tmp.path().join("noop.yaml");
    std::fs::write(
        &config,
        r#"
backend:
  url: https://backend.example.internal:8443
wrappers: {}
"#,
    )
    .unwrap();
    let cli = Cli {
        client_identity_out: None,
        instance_config: Some(config),
    };
    let error = cli.resolve_targets().unwrap_err();
    assert!(format!("{error:#}").contains("client_identity_file"));
}

#[test]
fn resolve_targets_requires_path_without_instance_config() {
    let cli = Cli {
        client_identity_out: None,
        instance_config: None,
    };
    let error = cli.resolve_targets().unwrap_err();
    assert!(format!("{error:#}").contains("--client-identity-out"));
}

#[derive(Debug, PartialEq, Eq)]
struct IdentitySnapshot {
    bytes: Vec<u8>,
    uid: u32,
    gid: u32,
    mode: u32,
    entries: BTreeSet<OsString>,
}

fn snapshot(path: &Path) -> IdentitySnapshot {
    let metadata = std::fs::metadata(path).unwrap();
    let entries = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    IdentitySnapshot {
        bytes: std::fs::read(path).unwrap(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        mode: metadata.permissions().mode() & 0o777,
        entries,
    }
}

fn with_install_failure<T>(phase: InstallPhase, action: impl FnOnce() -> T) -> T {
    let previous = FAIL_INSTALL_PHASE.with(|selected| selected.replace(Some(phase)));
    let result = action();
    FAIL_INSTALL_PHASE.with(|selected| selected.set(previous));
    result
}

#[test]
fn every_precommit_install_failure_preserves_old_identity_and_metadata() {
    for phase in [
        InstallPhase::Write,
        InstallPhase::Chown,
        InstallPhase::Chmod,
        InstallPhase::Sync,
        InstallPhase::Persist,
    ] {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("client.identity.pem");
        let (old_identity, _) = generate_key_and_self_signed_cert().unwrap();
        let (new_identity, _) = generate_key_and_self_signed_cert().unwrap();
        std::fs::write(&path, old_identity.as_slice()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o440)).unwrap();
        let before = snapshot(&path);

        let result = with_install_failure(phase, || {
            install_identity(
                &path,
                new_identity.as_slice(),
                Some((before.uid, before.gid)),
            )
        });

        assert!(result.is_err(), "{phase:?} failure unexpectedly succeeded");
        assert_eq!(snapshot(&path), before, "{phase:?} changed live identity");
    }
}

#[test]
fn successful_install_commits_one_complete_identity_with_exact_metadata() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("client.identity.pem");
    let (old_identity, _) = generate_key_and_self_signed_cert().unwrap();
    let (new_identity, new_cert) = generate_key_and_self_signed_cert().unwrap();
    std::fs::write(&path, old_identity.as_slice()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o440)).unwrap();
    let before = snapshot(&path);

    install_identity(
        &path,
        new_identity.as_slice(),
        Some((before.uid, before.gid)),
    )
    .unwrap();

    let after = snapshot(&path);
    assert_eq!(after.bytes, new_identity.as_slice());
    assert!(after.bytes.starts_with(new_cert.as_bytes()));
    assert!(
        std::str::from_utf8(&after.bytes)
            .unwrap()
            .contains("BEGIN PRIVATE KEY")
    );
    assert_eq!(
        (after.uid, after.gid, after.mode),
        (before.uid, before.gid, 0o400)
    );
    assert_eq!(after.entries, before.entries);
}

struct FailingWriter;

impl std::io::Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "injected stdout failure",
        ))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn stdout_failure_after_commit_does_not_restore_old_identity() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("client.identity.pem");
    let (old_identity, _) = generate_key_and_self_signed_cert().unwrap();
    let (new_identity, new_cert) = generate_key_and_self_signed_cert().unwrap();
    std::fs::write(&path, old_identity.as_slice()).unwrap();

    install_identity(&path, new_identity.as_slice(), None).unwrap();
    let error = write_public_certificate(&mut FailingWriter, &new_cert).unwrap_err();

    assert!(format!("{error:#}").contains("after identity commit"));
    assert_eq!(std::fs::read(path).unwrap(), new_identity.as_slice());
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

#[test]
fn resolve_target_owner_prefers_explicit_then_existing_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("client.identity.pem");

    assert!(resolve_target_owner(&path, None).is_none());

    std::fs::write(&path, b"x").unwrap();
    let metadata = std::fs::metadata(&path).unwrap();
    let (uid, gid) = (metadata.uid(), metadata.gid());
    assert_eq!(resolve_target_owner(&path, None), Some((uid, gid)));

    assert_eq!(
        resolve_target_owner(&path, Some((12345, 67890))),
        Some((12345, 67890))
    );
}
