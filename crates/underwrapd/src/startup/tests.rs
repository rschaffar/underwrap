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
fn backend_url_error_precedes_credential_shape_validation() {
    let mut config = minimal_config();
    config.backend.url = "http://plaintext.example.invalid".into();
    config.backend.client_cert_file = Some(std::path::PathBuf::from("/missing/client.pem"));
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

#[test]
fn validate_config_rejects_partial_backend_credentials() {
    let mut config = minimal_config();
    config.backend.client_cert_file = Some(std::path::PathBuf::from("/x/client.pem"));
    let err = validate_config(&config).expect_err("partial creds must fail");
    assert!(format!("{err:#}").contains("both be set or both be omitted"));
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
    fn missing_credential_files_are_tolerated() {
        let mut config = minimal_config();
        config.backend.client_cert_file = Some(std::path::PathBuf::from("/nonexistent/client.pem"));
        config.backend.client_key_file = Some(std::path::PathBuf::from("/nonexistent/client.key"));
        check_credential_permissions(&config).expect("missing files must be tolerated");
    }

    #[test]
    fn accepts_0600_owned_by_daemon() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let cert = dir.path().join("client.pem");
        let key = dir.path().join("client.key");
        write_mode(&cert, 0o600);
        write_mode(&key, 0o400);

        let mut config = minimal_config();
        config.backend.client_cert_file = Some(cert);
        config.backend.client_key_file = Some(key);
        check_credential_permissions(&config).expect("0600/0400 under 0700 dir must pass");
    }

    #[test]
    fn rejects_group_readable_key() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let cert = dir.path().join("client.pem");
        let key = dir.path().join("client.key");
        write_mode(&cert, 0o600);
        write_mode(&key, 0o640);

        let mut config = minimal_config();
        config.backend.client_cert_file = Some(cert);
        config.backend.client_key_file = Some(key);
        let err = check_credential_permissions(&config).expect_err("0640 key must be rejected");
        let msg = format!("{err:#}");
        assert!(msg.contains("0o640"), "unexpected error: {msg}");
        assert!(msg.contains("client key"), "unexpected error: {msg}");
    }

    #[test]
    fn rejects_world_accessible_parent_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let cert = dir.path().join("client.pem");
        let key = dir.path().join("client.key");
        write_mode(&cert, 0o600);
        write_mode(&key, 0o600);

        let mut config = minimal_config();
        config.backend.client_cert_file = Some(cert);
        config.backend.client_key_file = Some(key);
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
