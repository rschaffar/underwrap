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

// ===========================================================================
// Concurrent reads
// ===========================================================================
