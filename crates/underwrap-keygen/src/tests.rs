use super::*;
use clap::Parser;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::{Arc, Barrier};

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
    assert!(
        derive_instance_owner("definitely-not-an-existing-instance-xyzzy")
            .unwrap()
            .is_none()
    );
}

#[test]
fn derive_instance_owner_returns_none_for_empty_instance() {
    assert!(derive_instance_owner("").unwrap().is_none());
    assert!(derive_instance_owner("   ").unwrap().is_none());
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

#[derive(Debug)]
struct TestNssRecord {
    id: u32,
}

#[derive(Clone, Copy, Debug)]
enum LookupStep {
    Erange,
    Error(libc::c_int),
    NotFound,
    Success(u32),
    ForeignPointer,
}

#[derive(Debug)]
struct LookupEvidence {
    result: Result<Option<u32>>,
    reserve_targets: Vec<usize>,
    lookup_lengths: Vec<usize>,
}

fn injected_lookup(
    name: &str,
    hint: NssBufferHint,
    limits: NssBufferLimits,
    steps: impl IntoIterator<Item = LookupStep>,
    fail_reserve_call: Option<usize>,
) -> LookupEvidence {
    let reserve_targets = RefCell::new(Vec::new());
    let lookup_lengths = RefCell::new(Vec::new());
    let steps = RefCell::new(steps.into_iter().collect::<VecDeque<_>>());
    let reserve_calls = Cell::new(0usize);
    let result = lookup_account::<TestNssRecord>(
        name,
        "test",
        hint,
        limits,
        |buffer, additional| {
            let call = reserve_calls.get() + 1;
            reserve_calls.set(call);
            reserve_targets
                .borrow_mut()
                .push(buffer.len().checked_add(additional).unwrap());
            if fail_reserve_call == Some(call) {
                bail!("injected NSS allocation failure at reservation {call}");
            }
            reserve_nss_buffer(buffer, additional)
        },
        |_name, output, _buffer, buffer_len, result| {
            lookup_lengths.borrow_mut().push(buffer_len);
            match steps
                .borrow_mut()
                .pop_front()
                .expect("injected lookup step")
            {
                LookupStep::Erange => libc::ERANGE,
                LookupStep::Error(error) => error,
                LookupStep::NotFound => 0,
                LookupStep::Success(id) => {
                    // SAFETY: the production seam supplied valid caller-owned output
                    // and result pointers for this synchronous injected lookup.
                    unsafe {
                        output.write(TestNssRecord { id });
                        result.write(output);
                    }
                    0
                }
                LookupStep::ForeignPointer => {
                    // SAFETY: `result` is a valid caller-owned result slot. The
                    // deliberately foreign pointer is compared but never dereferenced.
                    unsafe { result.write(output.wrapping_add(1)) };
                    0
                }
            }
        },
        |record: &TestNssRecord| record.id,
    );
    LookupEvidence {
        result,
        reserve_targets: reserve_targets.into_inner(),
        lookup_lengths: lookup_lengths.into_inner(),
    }
}

#[test]
fn unavailable_zero_and_negative_hints_use_the_16_kib_fallback() {
    for hint in [
        NssBufferHint::Unavailable,
        NssBufferHint::Value(0),
        NssBufferHint::Value(-17),
    ] {
        let evidence = injected_lookup(
            "missing",
            hint,
            NSS_BUFFER_LIMITS,
            [LookupStep::NotFound],
            None,
        );
        assert_eq!(evidence.result.unwrap(), None);
        assert_eq!(evidence.reserve_targets, [NSS_BUFFER_FALLBACK]);
        assert_eq!(evidence.lookup_lengths, [NSS_BUFFER_FALLBACK]);
    }
}

#[test]
fn unrepresentable_and_oversized_hints_abort_before_allocation_or_lookup() {
    let unrepresentable = injected_lookup(
        "account",
        NssBufferHint::Value((usize::MAX as i128) + 1),
        NSS_BUFFER_LIMITS,
        [],
        None,
    );
    assert!(format!("{:#}", unrepresentable.result.unwrap_err()).contains("not representable"));
    assert!(unrepresentable.reserve_targets.is_empty());
    assert!(unrepresentable.lookup_lengths.is_empty());

    let oversized = injected_lookup(
        "account",
        NssBufferHint::Value((NSS_BUFFER_MAX + 1) as i128),
        NSS_BUFFER_LIMITS,
        [],
        None,
    );
    assert!(format!("{:#}", oversized.result.unwrap_err()).contains("exceeds"));
    assert!(oversized.reserve_targets.is_empty());
    assert!(oversized.lookup_lengths.is_empty());
}

#[test]
fn erange_alone_grows_by_checked_doubling_and_success_copies_the_id() {
    let evidence = injected_lookup(
        "account",
        NssBufferHint::Value(NSS_BUFFER_FALLBACK as i128),
        NSS_BUFFER_LIMITS,
        [LookupStep::Erange, LookupStep::Success(4242)],
        None,
    );
    assert_eq!(evidence.result.unwrap(), Some(4242));
    assert_eq!(
        evidence.reserve_targets,
        [NSS_BUFFER_FALLBACK, NSS_BUFFER_FALLBACK * 2]
    );
    assert_eq!(
        evidence.lookup_lengths,
        [NSS_BUFFER_FALLBACK, NSS_BUFFER_FALLBACK * 2]
    );
}

#[test]
fn injected_max_caps_growth_and_erange_at_the_max_aborts() {
    let limits = NssBufferLimits {
        fallback: 8,
        max: 16,
    };
    let evidence = injected_lookup(
        "account",
        NssBufferHint::Unavailable,
        limits,
        [LookupStep::Erange, LookupStep::Erange],
        None,
    );
    assert!(format!("{:#}", evidence.result.unwrap_err()).contains("16-byte buffer maximum"));
    assert_eq!(evidence.reserve_targets, [8, 16]);
    assert_eq!(evidence.lookup_lengths, [8, 16]);

    let production_max = injected_lookup(
        "account",
        NssBufferHint::Value(NSS_BUFFER_MAX as i128),
        NSS_BUFFER_LIMITS,
        [LookupStep::Erange],
        None,
    );
    assert!(
        format!("{:#}", production_max.result.unwrap_err()).contains("1048576-byte buffer maximum")
    );
    assert_eq!(production_max.lookup_lengths, [NSS_BUFFER_MAX]);
}

#[test]
fn growth_arithmetic_is_checked() {
    let error = next_nss_buffer_size(usize::MAX / 2 + 1, usize::MAX).unwrap_err();
    assert!(format!("{error:#}").contains("arithmetic overflow"));
}

#[test]
fn initial_and_growth_allocation_failures_abort_without_extra_lookup() {
    let initial = injected_lookup(
        "account",
        NssBufferHint::Unavailable,
        NSS_BUFFER_LIMITS,
        [],
        Some(1),
    );
    assert!(format!("{:#}", initial.result.unwrap_err()).contains("allocation failure"));
    assert!(initial.lookup_lengths.is_empty());

    let growth = injected_lookup(
        "account",
        NssBufferHint::Unavailable,
        NSS_BUFFER_LIMITS,
        [LookupStep::Erange],
        Some(2),
    );
    assert!(format!("{:#}", growth.result.unwrap_err()).contains("allocation failure"));
    assert_eq!(growth.lookup_lengths, [NSS_BUFFER_FALLBACK]);
    assert_eq!(
        growth.reserve_targets,
        [NSS_BUFFER_FALLBACK, NSS_BUFFER_FALLBACK * 2]
    );
}

#[test]
fn eintr_and_other_errors_propagate_without_retry_or_growth() {
    for error in [libc::EINTR, libc::EIO] {
        let evidence = injected_lookup(
            "account",
            NssBufferHint::Unavailable,
            NSS_BUFFER_LIMITS,
            [LookupStep::Error(error), LookupStep::Success(99)],
            None,
        );
        let message = format!("{:#}", evidence.result.unwrap_err());
        assert!(message.contains("through NSS"));
        assert_eq!(evidence.reserve_targets, [NSS_BUFFER_FALLBACK]);
        assert_eq!(evidence.lookup_lengths, [NSS_BUFFER_FALLBACK]);
    }
}

#[test]
fn not_found_is_the_only_null_result_fallback_and_foreign_pointer_is_rejected() {
    let not_found = injected_lookup(
        "missing",
        NssBufferHint::Unavailable,
        NSS_BUFFER_LIMITS,
        [LookupStep::NotFound],
        None,
    );
    assert_eq!(not_found.result.unwrap(), None);

    let foreign = injected_lookup(
        "account",
        NssBufferHint::Unavailable,
        NSS_BUFFER_LIMITS,
        [LookupStep::ForeignPointer],
        None,
    );
    assert!(format!("{:#}", foreign.result.unwrap_err()).contains("unexpected result pointer"));
}

#[test]
fn invalid_lookup_input_aborts_before_allocation_or_nss_call() {
    let evidence = injected_lookup(
        "invalid\0account",
        NssBufferHint::Unavailable,
        NSS_BUFFER_LIMITS,
        [],
        None,
    );
    assert!(format!("{:#}", evidence.result.unwrap_err()).contains("interior NUL"));
    assert!(evidence.reserve_targets.is_empty());
    assert!(evidence.lookup_lengths.is_empty());
}

#[test]
fn real_reentrant_nss_lookup_finds_root_and_preserves_not_found() {
    assert_eq!(lookup_uid("root").unwrap(), Some(0));
    assert_eq!(lookup_gid("root").unwrap(), Some(0));
    let missing = format!("underwrap-keygen-missing-account-{}", std::process::id());
    assert_eq!(lookup_uid(&missing).unwrap(), None);
    assert_eq!(lookup_gid(&missing).unwrap(), None);
}

#[derive(Debug)]
struct FixedResolver {
    uid: Result<Option<u32>>,
    gid: Result<Option<u32>>,
    names: Vec<String>,
}

impl AccountResolver for FixedResolver {
    fn lookup_uid(&mut self, name: &str) -> Result<Option<u32>> {
        self.names.push(format!("uid:{name}"));
        std::mem::replace(&mut self.uid, Ok(None))
    }

    fn lookup_gid(&mut self, name: &str) -> Result<Option<u32>> {
        self.names.push(format!("gid:{name}"));
        std::mem::replace(&mut self.gid, Ok(None))
    }
}

#[test]
fn daemon_owner_resolution_preserves_success_and_not_found_precedence() {
    let mut resolved = FixedResolver {
        uid: Ok(Some(123)),
        gid: Ok(Some(456)),
        names: Vec::new(),
    };
    assert_eq!(
        derive_instance_owner_with("work", &mut resolved).unwrap(),
        Some((123, 456))
    );
    assert_eq!(resolved.names, ["uid:underwrap-work", "gid:underwrap-work"]);

    let mut missing_user = FixedResolver {
        uid: Ok(None),
        gid: Err(anyhow::anyhow!("group lookup must not run")),
        names: Vec::new(),
    };
    assert_eq!(
        derive_instance_owner_with("work", &mut missing_user).unwrap(),
        None
    );
    assert_eq!(missing_user.names, ["uid:underwrap-work"]);

    let mut missing_group = FixedResolver {
        uid: Ok(Some(123)),
        gid: Ok(None),
        names: Vec::new(),
    };
    assert_eq!(
        derive_instance_owner_with("work", &mut missing_group).unwrap(),
        None
    );
    assert_eq!(
        missing_group.names,
        ["uid:underwrap-work", "gid:underwrap-work"]
    );
}

#[test]
fn every_nss_operational_failure_precedes_generation_and_file_mutation() {
    let failures = [
        "unrepresentable hint",
        "oversized hint",
        "initial allocation",
        "maximum ERANGE",
        "growth arithmetic",
        "growth allocation",
        "EINTR",
        "other NSS error",
        "invalid input",
        "foreign result pointer",
    ];
    for failure in failures {
        let tmp = tempfile::TempDir::new().unwrap();
        let identity = tmp.path().join("client.identity.pem");
        std::fs::write(&identity, b"unchanged identity").unwrap();
        std::fs::set_permissions(&identity, std::fs::Permissions::from_mode(0o440)).unwrap();
        let config = tmp.path().join("work.yaml");
        std::fs::write(
            &config,
            format!(
                "backend:\n  url: https://backend.example.internal:8443\n  client_identity_file: {}\n",
                identity.display()
            ),
        )
        .unwrap();
        let before = snapshot(&identity);
        let generated = Cell::new(false);
        let mut resolver = FixedResolver {
            uid: Err(anyhow::anyhow!("injected {failure}")),
            gid: Ok(Some(456)),
            names: Vec::new(),
        };
        let error = run_after_hardening(
            Cli {
                client_identity_out: None,
                instance_config: Some(config),
            },
            &mut resolver,
            || {
                generated.set(true);
                generate_key_and_self_signed_cert()
            },
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains(failure));
        assert!(!generated.get(), "{failure} reached key generation");
        assert_eq!(
            snapshot(&identity),
            before,
            "{failure} mutated the identity"
        );
    }
}

#[test]
fn public_resolve_targets_returns_stable_results_when_calls_start_concurrently() {
    const THREADS: usize = 8;
    const CALLS_PER_THREAD: usize = 32;

    let tmp = tempfile::TempDir::new().unwrap();
    let config = tmp.path().join(format!(
        "definitely-missing-concurrent-{}.yaml",
        std::process::id()
    ));
    let identity = tmp.path().join("client.identity.pem");
    std::fs::write(
        &config,
        format!(
            "backend:\n  url: https://backend.example.internal:8443\n  client_identity_file: {}\n",
            identity.display()
        ),
    )
    .unwrap();
    let cli = Arc::new(Cli {
        client_identity_out: None,
        instance_config: Some(config),
    });
    let barrier = Arc::new(Barrier::new(THREADS));
    let threads: Vec<_> = (0..THREADS)
        .map(|_| {
            let cli = Arc::clone(&cli);
            let barrier = Arc::clone(&barrier);
            let identity = identity.clone();
            std::thread::spawn(move || {
                barrier.wait();
                for _ in 0..CALLS_PER_THREAD {
                    let targets = cli.resolve_targets().unwrap();
                    assert_eq!(targets.client_identity_out, identity);
                    assert_eq!(targets.credential_owner, None);
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}
