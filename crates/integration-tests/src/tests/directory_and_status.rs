use super::*;

#[tokio::test]
async fn overlapping_directory_paths_rejected_at_startup() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let wrapper_socket = base.join("wrapper.sock");

    let mut credentials = HashMap::new();
    credentials.insert("gws/state".to_string(), directory_credential(".config/gws"));
    credentials.insert(
        "gws/state-nested".to_string(),
        directory_credential(".config/gws/session"),
    );

    let mut use_map = HashMap::new();
    use_map.insert(
        "gws/state".to_string(),
        underwrap_protocol::config::CredentialUse {
            access: underwrap_protocol::config::AccessMode::ReadWrite,
        },
    );
    use_map.insert(
        "gws/state-nested".to_string(),
        underwrap_protocol::config::CredentialUse {
            access: underwrap_protocol::config::AccessMode::ReadWrite,
        },
    );

    let mut wrapper_def = wrapper("/usr/bin/true", "gws/state", "IGNORED");
    wrapper_def.use_map = use_map;
    wrapper_def.socket = wrapper_socket;

    let err = daemon_config_error(credentials, wrapper_def).await;
    assert!(
        err.to_string()
            .contains("overlapping directory credential paths")
    );
}

#[tokio::test]
async fn duplicate_directory_paths_rejected_at_startup() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let wrapper_socket = base.join("wrapper.sock");

    let mut credentials = HashMap::new();
    credentials.insert(
        "gws/state-a".to_string(),
        directory_credential(".config/gws"),
    );
    credentials.insert(
        "gws/state-b".to_string(),
        directory_credential(".config/gws"),
    );

    let mut use_map = HashMap::new();
    for cred in ["gws/state-a", "gws/state-b"] {
        use_map.insert(
            cred.to_string(),
            underwrap_protocol::config::CredentialUse {
                access: underwrap_protocol::config::AccessMode::ReadWrite,
            },
        );
    }

    let mut wrapper_def = wrapper("/usr/bin/true", "gws/state-a", "IGNORED");
    wrapper_def.use_map = use_map;
    wrapper_def.socket = wrapper_socket;

    let err = daemon_config_error(credentials, wrapper_def).await;
    assert!(err.contains("overlapping directory credential paths"));
}

#[tokio::test]
async fn absolute_directory_path_rejected_at_startup() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let wrapper_socket = base.join("wrapper.sock");

    let credentials = HashMap::from([("gws/state".to_string(), directory_credential("/etc/gws"))]);

    let mut wrapper_def = wrapper("/usr/bin/true", "gws/state", "IGNORED");
    wrapper_def.use_map.insert(
        "gws/state".to_string(),
        underwrap_protocol::config::CredentialUse {
            access: underwrap_protocol::config::AccessMode::ReadWrite,
        },
    );
    wrapper_def.socket = wrapper_socket;

    let err = daemon_config_error(credentials, wrapper_def).await;
    assert!(err.contains("invalid directory path") || err.contains("path must be relative"));
}

#[tokio::test]
async fn traversal_directory_path_rejected_at_startup() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let wrapper_socket = base.join("wrapper.sock");

    let credentials = HashMap::from([(
        "gws/state".to_string(),
        directory_credential(".config/../gws"),
    )]);

    let mut wrapper_def = wrapper("/usr/bin/true", "gws/state", "IGNORED");
    wrapper_def.use_map.insert(
        "gws/state".to_string(),
        underwrap_protocol::config::CredentialUse {
            access: underwrap_protocol::config::AccessMode::ReadWrite,
        },
    );
    wrapper_def.socket = wrapper_socket;

    let err = daemon_config_error(credentials, wrapper_def).await;
    assert!(err.contains("invalid directory path") || err.contains("path must be clean"));
}

#[tokio::test]
async fn empty_segment_directory_path_rejected_at_startup() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let wrapper_socket = base.join("wrapper.sock");

    let credentials = HashMap::from([(
        "gws/state".to_string(),
        directory_credential(".config//gws"),
    )]);

    let mut wrapper_def = wrapper("/usr/bin/true", "gws/state", "IGNORED");
    wrapper_def.use_map.insert(
        "gws/state".to_string(),
        underwrap_protocol::config::CredentialUse {
            access: underwrap_protocol::config::AccessMode::ReadWrite,
        },
    );
    wrapper_def.socket = wrapper_socket;

    let err = daemon_config_error(credentials, wrapper_def).await;
    assert!(err.contains("invalid directory path") || err.contains("empty segments"));
}

#[tokio::test]
async fn directory_bindings_are_returned_on_read() {
    let mut wrappers = HashMap::new();
    let mut use_map = HashMap::new();
    use_map.insert(
        "gws/state-a".to_string(),
        underwrap_protocol::config::CredentialUse {
            access: underwrap_protocol::config::AccessMode::ReadOnly,
        },
    );
    use_map.insert(
        "gws/state-b".to_string(),
        underwrap_protocol::config::CredentialUse {
            access: underwrap_protocol::config::AccessMode::ReadWrite,
        },
    );
    let mut wrapper_def = wrapper("/usr/bin/true", "gws/state-a", "IGNORED");
    wrapper_def.use_map = use_map;
    wrappers.insert("dir-w".to_string(), wrapper_def);

    let credentials = HashMap::from([
        (
            "gws/state-a".to_string(),
            directory_credential(".config/gws-a"),
        ),
        (
            "gws/state-b".to_string(),
            directory_credential(".local/share/gws-b"),
        ),
    ]);

    let daemon = TestDaemon::start(wrappers, credentials).await.unwrap();
    let resp = daemon.read_request("dir-w").await.unwrap();
    match resp {
        WrapperResponse::ReadOk { env, directories } => {
            assert!(env.is_empty());
            assert_eq!(directories.len(), 2);
            assert_eq!(directories[0].target_path, ".config/gws-a");
            assert_eq!(directories[1].target_path, ".local/share/gws-b");
        }
        other => panic!("expected ReadOk, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn directory_status_reports_stored_state() {
    let mut wrappers = HashMap::new();
    wrappers.insert(
        "dir-w".to_string(),
        wrapper("/usr/bin/true", "gws/state", "IGNORED"),
    );
    let credentials =
        HashMap::from([("gws/state".to_string(), directory_credential(".config/gws"))]);
    let daemon = TestDaemon::start(wrappers, credentials).await.unwrap();

    let dir = daemon.storage_dir.join("gws/state/files");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/token"), b"abc").unwrap();

    let status = expect_credential_status(daemon.status(Some("gws/state")).await.unwrap());
    assert!(status.stored);

    let wrapper_status = expect_wrapper_status(daemon.status(Some("dir-w")).await.unwrap());
    assert_eq!(wrapper_status.usage[0].stored, Some(true));

    daemon.stop().await.unwrap();
}

#[tokio::test]
async fn status_general() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let resp = daemon.status(None).await.unwrap();
    let (wrappers, credentials) = expect_status_all(resp);
    assert_eq!(wrappers.len(), 1);
    assert_eq!(credentials.len(), 1);

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Status — specific wrapper
// ===========================================================================

#[tokio::test]
async fn status_specific_wrapper() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    // Before storing: usage[0].stored = false.
    let resp = daemon.status(Some("gh-authorized")).await.unwrap();
    let status = expect_wrapper_status(resp);
    assert_eq!(status.wrapper, "gh-authorized");
    assert_eq!(status.usage[0].credential, "github/token");
    assert_eq!(status.usage[0].stored, Some(false));

    // After storing: usage[0].stored = true.
    daemon.store_secret("github/token", "tok").await.unwrap();
    let resp = daemon.status(Some("gh-authorized")).await.unwrap();
    let status = expect_wrapper_status(resp);
    assert_eq!(status.usage[0].stored, Some(true));

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Status — specific secret
// ===========================================================================

#[tokio::test]
async fn status_specific_secret() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let resp = daemon.status(Some("github/token")).await.unwrap();
    let status = expect_credential_status(resp);
    assert_eq!(status.credential, "github/token");
    assert!(!status.stored);

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Status — unknown target
// ===========================================================================

#[tokio::test]
async fn status_unknown_target() {
    let (wrappers, secrets) = one_wrapper_config();
    let daemon = TestDaemon::start(wrappers, secrets).await.unwrap();

    let resp = daemon.status(Some("nope")).await.unwrap();
    match resp {
        ControlResponse::Error { code, message } => {
            assert!(matches!(code, ErrorCode::NotFound));
            assert!(message.contains("nope"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    daemon.stop().await.unwrap();
}

// ===========================================================================
// Concurrent reads
// ===========================================================================
