use super::*;
use crate::test_support::config_with_backend_url;

fn minimal_config() -> DaemonConfig {
    let mut config = config_with_backend_url("https://backend.example.internal");
    config.backend.ca_file = Some(std::path::PathBuf::from("/etc/underwrap/ca.pem"));
    config
}

#[test]
fn paths_overlap_detects_parent_child_and_equal() {
    assert!(paths_overlap(".config/gws", ".config/gws"));
    assert!(paths_overlap(".config/gws", ".config/gws/session"));
    assert!(paths_overlap(".config/gws/session", ".config/gws"));
    assert!(!paths_overlap(".config/gws", ".config/gws2"));
    assert!(!paths_overlap(".config/a", ".local/a"));
}

#[test]
fn backend_url_accepts_absolute_https_hosts_and_proxy_base_paths() {
    for value in [
        "https://backend.example.internal",
        "https://proxy.example.internal:8443/base/",
        "https://127.0.0.1:9443/runtime/prefix/",
    ] {
        let parsed = validate_backend_url(value).unwrap();
        assert_eq!(parsed.scheme(), "https");
        assert!(parsed.host().is_some());
    }
}

#[test]
fn backend_url_rejects_empty_malformed_relative_hostless_and_non_https() {
    for value in [
        "",
        "not a url",
        "/relative/backend",
        "https://",
        "http://backend.example.internal",
        "ftp://backend.example.internal",
        "file:///tmp/backend",
    ] {
        let error = validate_backend_url(value).expect_err("URL must be rejected");
        assert_eq!(
            error.to_string(),
            "backend.url must be an absolute HTTPS URL with a host"
        );
    }
}

#[test]
fn backend_url_error_precedes_credential_validation() {
    let mut config = minimal_config();
    config.backend.url = "http://plaintext.example.invalid".into();
    config.backend.client_identity_file =
        Some(std::path::PathBuf::from("/missing/client.identity.pem"));
    let error = validate_config(&config).expect_err("plaintext URL must fail first");
    assert_eq!(
        error.to_string(),
        "backend.url must be an absolute HTTPS URL with a host"
    );
}

#[test]
fn validate_config_allows_unenrolled_backend_credentials() {
    validate_config(&minimal_config()).unwrap();
}

mod group_resolution {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, Ordering};

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
        hint: GroupBufferHint,
        limits: GroupBufferLimits,
        steps: impl IntoIterator<Item = LookupStep>,
        fail_reserve_call: Option<usize>,
    ) -> LookupEvidence {
        let reserve_targets = RefCell::new(Vec::new());
        let lookup_lengths = RefCell::new(Vec::new());
        let steps = RefCell::new(steps.into_iter().collect::<VecDeque<_>>());
        let reserve_calls = Cell::new(0usize);
        let result = resolve_group_gid_with(
            name,
            hint,
            limits,
            |buffer, additional| {
                let call = reserve_calls.get() + 1;
                reserve_calls.set(call);
                let target = buffer.len().checked_add(additional).unwrap();
                assert!(target <= GROUP_BUFFER_MAX);
                reserve_targets.borrow_mut().push(target);
                if fail_reserve_call == Some(call) {
                    anyhow::bail!("injected group NSS allocation failure at reservation {call}");
                }
                reserve_group_buffer(buffer, additional)
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
                    LookupStep::Success(gid) => {
                        // SAFETY: an all-zero `libc::group` has valid integer and
                        // null-pointer fields. The production seam supplied valid
                        // caller-owned output and result pointers, which remain live.
                        unsafe {
                            let mut group = std::mem::zeroed::<libc::group>();
                            group.gr_gid = gid;
                            output.write(group);
                            result.write(output);
                        }
                        0
                    }
                    LookupStep::ForeignPointer => {
                        // SAFETY: `result` is a valid caller-owned result slot. The
                        // foreign pointer is compared but never dereferenced.
                        unsafe { result.write(output.cast::<u8>().wrapping_add(1).cast()) };
                        0
                    }
                }
            },
        );
        LookupEvidence {
            result,
            reserve_targets: reserve_targets.into_inner(),
            lookup_lengths: lookup_lengths.into_inner(),
        }
    }

    fn missing_group_name(label: &str) -> String {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        format!(
            "underwrap_missing_{label}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[test]
    fn unavailable_zero_and_distinct_negative_hints_use_the_16_kib_fallback() {
        for hint in [
            GroupBufferHint::Unavailable,
            GroupBufferHint::Value(0),
            GroupBufferHint::Value(-23),
        ] {
            let evidence = injected_lookup(
                "missing",
                hint,
                GROUP_BUFFER_LIMITS,
                [LookupStep::NotFound],
                None,
            );
            assert_eq!(evidence.result.unwrap(), None);
            assert_eq!(evidence.reserve_targets, [GROUP_BUFFER_FALLBACK]);
            assert_eq!(evidence.lookup_lengths, [GROUP_BUFFER_FALLBACK]);
        }
    }

    #[test]
    fn unrepresentable_and_oversized_hints_abort_before_allocation_or_lookup() {
        let unrepresentable = injected_lookup(
            "group",
            GroupBufferHint::Value((usize::MAX as i128) + 1),
            GROUP_BUFFER_LIMITS,
            [],
            None,
        );
        assert!(format!("{:#}", unrepresentable.result.unwrap_err()).contains("not representable"));
        assert!(unrepresentable.reserve_targets.is_empty());
        assert!(unrepresentable.lookup_lengths.is_empty());

        let oversized = injected_lookup(
            "group",
            GroupBufferHint::Value((GROUP_BUFFER_MAX + 1) as i128),
            GROUP_BUFFER_LIMITS,
            [],
            None,
        );
        assert!(format!("{:#}", oversized.result.unwrap_err()).contains("exceeds"));
        assert!(oversized.reserve_targets.is_empty());
        assert!(oversized.lookup_lengths.is_empty());
    }

    #[test]
    fn erange_alone_doubles_the_buffer_exactly() {
        let evidence = injected_lookup(
            "group",
            GroupBufferHint::Value(GROUP_BUFFER_FALLBACK as i128),
            GROUP_BUFFER_LIMITS,
            [LookupStep::Erange, LookupStep::Success(4242)],
            None,
        );
        assert_eq!(evidence.result.unwrap(), Some(4242));
        assert_eq!(
            evidence.reserve_targets,
            [GROUP_BUFFER_FALLBACK, GROUP_BUFFER_FALLBACK * 2]
        );
        assert_eq!(
            evidence.lookup_lengths,
            [GROUP_BUFFER_FALLBACK, GROUP_BUFFER_FALLBACK * 2]
        );
    }

    #[test]
    fn growth_caps_at_one_mib_and_can_succeed_at_the_maximum() {
        let initial = GROUP_BUFFER_MAX * 3 / 4;
        let evidence = injected_lookup(
            "group",
            GroupBufferHint::Value(initial as i128),
            GROUP_BUFFER_LIMITS,
            [LookupStep::Erange, LookupStep::Success(5150)],
            None,
        );
        assert_eq!(evidence.result.unwrap(), Some(5150));
        assert_eq!(evidence.reserve_targets, [initial, GROUP_BUFFER_MAX]);
        assert_eq!(evidence.lookup_lengths, [initial, GROUP_BUFFER_MAX]);
    }

    #[test]
    fn erange_at_one_mib_aborts_without_another_lookup() {
        let evidence = injected_lookup(
            "group",
            GroupBufferHint::Value(GROUP_BUFFER_MAX as i128),
            GROUP_BUFFER_LIMITS,
            [LookupStep::Erange],
            None,
        );
        assert!(
            format!("{:#}", evidence.result.unwrap_err()).contains("1048576-byte buffer maximum")
        );
        assert_eq!(evidence.reserve_targets, [GROUP_BUFFER_MAX]);
        assert_eq!(evidence.lookup_lengths, [GROUP_BUFFER_MAX]);
    }

    #[test]
    fn growth_arithmetic_is_checked() {
        let error = next_group_buffer_size((usize::MAX / 2) + 1, usize::MAX).unwrap_err();
        assert!(format!("{error:#}").contains("arithmetic overflow"));
    }

    #[test]
    fn initial_and_growth_reservation_failures_abort_at_the_failing_request() {
        let initial = injected_lookup(
            "group",
            GroupBufferHint::Unavailable,
            GROUP_BUFFER_LIMITS,
            [],
            Some(1),
        );
        assert!(format!("{:#}", initial.result.unwrap_err()).contains("reservation 1"));
        assert_eq!(initial.reserve_targets, [GROUP_BUFFER_FALLBACK]);
        assert!(initial.lookup_lengths.is_empty());

        let limits = GroupBufferLimits {
            fallback: 8,
            max: 16,
        };
        let growth = injected_lookup(
            "group",
            GroupBufferHint::Unavailable,
            limits,
            [LookupStep::Erange],
            Some(2),
        );
        assert!(format!("{:#}", growth.result.unwrap_err()).contains("reservation 2"));
        assert_eq!(growth.reserve_targets, [8, 16]);
        assert_eq!(growth.lookup_lengths, [8]);
    }

    #[test]
    fn non_erange_errors_propagate_without_retry_or_growth() {
        for error in [libc::EINTR, libc::EIO] {
            let evidence = injected_lookup(
                "group",
                GroupBufferHint::Unavailable,
                GROUP_BUFFER_LIMITS,
                [LookupStep::Error(error), LookupStep::Success(1)],
                None,
            );
            let propagated = evidence.result.unwrap_err();
            assert_eq!(
                propagated
                    .root_cause()
                    .downcast_ref::<std::io::Error>()
                    .and_then(std::io::Error::raw_os_error),
                Some(error)
            );
            assert_eq!(evidence.reserve_targets, [GROUP_BUFFER_FALLBACK]);
            assert_eq!(evidence.lookup_lengths, [GROUP_BUFFER_FALLBACK]);
        }
    }

    #[test]
    fn embedded_nul_aborts_before_allocation_or_lookup() {
        let evidence = injected_lookup(
            "bad\0group",
            GroupBufferHint::Unavailable,
            GROUP_BUFFER_LIMITS,
            [],
            None,
        );
        assert_eq!(
            evidence.result.unwrap_err().to_string(),
            "group name 'bad\0group' contains null byte"
        );
        assert!(evidence.reserve_targets.is_empty());
        assert!(evidence.lookup_lengths.is_empty());
    }

    #[test]
    fn null_result_is_ordinary_not_found() {
        let evidence = injected_lookup(
            "missing",
            GroupBufferHint::Unavailable,
            GROUP_BUFFER_LIMITS,
            [LookupStep::NotFound],
            None,
        );
        assert_eq!(evidence.result.unwrap(), None);
    }

    #[test]
    fn exact_result_pointer_copies_only_the_initialized_gid() {
        let evidence = injected_lookup(
            "group",
            GroupBufferHint::Unavailable,
            GROUP_BUFFER_LIMITS,
            [LookupStep::Success(8675)],
            None,
        );
        assert_eq!(evidence.result.unwrap(), Some(8675));
    }

    #[test]
    fn foreign_result_pointer_is_rejected_without_dereference() {
        let evidence = injected_lookup(
            "group",
            GroupBufferHint::Unavailable,
            GROUP_BUFFER_LIMITS,
            [LookupStep::ForeignPointer],
            None,
        );
        assert!(
            format!("{:#}", evidence.result.unwrap_err()).contains("unexpected result pointer")
        );
    }

    #[test]
    fn real_lookup_resolves_root_and_reports_a_generated_missing_group() {
        assert_eq!(resolve_group_gid("root").unwrap(), 0);
        let missing = missing_group_name("real");
        assert_eq!(
            resolve_group_gid(&missing).unwrap_err().to_string(),
            format!("group '{missing}' not found")
        );
    }

    #[tokio::test]
    async fn server_run_missing_group_fails_before_creating_the_wrapper_socket() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("wrapper.sock");
        let missing = missing_group_name("server");
        let mut config = minimal_config();
        config.socket = socket.clone();
        config.group = missing.clone();

        let error = crate::server::run(config, None)
            .await
            .expect_err("missing instance group must stop daemon startup");
        assert!(
            format!("{error:#}").contains(&format!("group '{missing}' not found")),
            "unexpected error: {error:#}",
        );
        assert!(!socket.exists(), "wrapper socket must not be created");
    }
}

mod credential_permissions {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn write_mode(path: &std::path::Path, mode: u32) {
        std::fs::write(path, b"x").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// Absent files must not fail (the daemon self-heals before keygen runs).
    #[test]
    fn missing_credential_file_is_tolerated() {
        let mut config = minimal_config();
        config.backend.client_identity_file =
            Some(std::path::PathBuf::from("/nonexistent/client.identity.pem"));
        check_credential_permissions(&config).expect("missing file must be tolerated");
    }

    #[test]
    fn accepts_0400_identity_owned_by_daemon() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let identity = dir.path().join("client.identity.pem");
        write_mode(&identity, 0o400);

        let mut config = minimal_config();
        config.backend.client_identity_file = Some(identity);
        check_credential_permissions(&config).expect("0400 identity under 0700 dir must pass");
    }

    #[test]
    fn rejects_group_readable_identity() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let identity = dir.path().join("client.identity.pem");
        write_mode(&identity, 0o640);

        let mut config = minimal_config();
        config.backend.client_identity_file = Some(identity);
        let err =
            check_credential_permissions(&config).expect_err("0640 identity must be rejected");
        let msg = format!("{err:#}");
        assert!(msg.contains("0o640"), "unexpected error: {msg}");
        assert!(msg.contains("client identity"), "unexpected error: {msg}");
    }

    #[test]
    fn rejects_world_accessible_parent_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let identity = dir.path().join("client.identity.pem");
        write_mode(&identity, 0o400);

        let mut config = minimal_config();
        config.backend.client_identity_file = Some(identity);
        let err =
            check_credential_permissions(&config).expect_err("0755 parent dir must be rejected");
        assert!(
            format!("{err:#}").contains("parent directory"),
            "unexpected error: {err:#}",
        );
    }
}

mod socket_binding {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    // The socket inherits the daemon's primary group (the instance group), and
    // `bind_socket` verifies that GID. These tests exercise both parts of that
    // contract on a real Unix socket.

    #[tokio::test]
    async fn socket_inherits_caller_uid_and_gid_and_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrapper.sock");
        let egid = unsafe { libc::getegid() };

        // Passing the caller's egid as the expected gid models the
        // production invariant (daemon primary group == instance group).
        let _listener = bind_socket(&path, 0o660, "test", Some(egid)).expect("bind must succeed");

        let meta = std::fs::metadata(&path).unwrap();
        assert_eq!(
            meta.uid(),
            unsafe { libc::geteuid() },
            "socket owner must be the binding process's uid",
        );
        assert_eq!(
            meta.gid(),
            egid,
            "socket group must be the binding process's primary group",
        );
        assert_eq!(
            meta.permissions().mode() & 0o777,
            0o660,
            "socket mode must be 0660",
        );
    }

    #[tokio::test]
    async fn rejects_socket_with_unexpected_group() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wrapper.sock");
        // A gid the socket cannot possibly have been created with (the
        // socket's group is always the caller's egid). bind_socket must
        // fail closed rather than serve a wrongly-grouped socket.
        let wrong_gid = unsafe { libc::getegid() }.wrapping_add(424_242);

        let err = bind_socket(&path, 0o660, "test", Some(wrong_gid))
            .expect_err("wrong instance group must be rejected");
        assert!(
            format!("{err:#}").contains("wrongly-grouped socket"),
            "unexpected error: {err:#}",
        );
    }
}
