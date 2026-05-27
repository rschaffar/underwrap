use super::*;

#[tokio::test]
async fn overlapping_directory_paths_rejected_at_startup() {
    let wrapper_def = files_wrapper_paths(
        "/usr/bin/true",
        "gws/state",
        &[".config/gws", ".config/gws/session"],
    );

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("overlapping files paths"));
}

#[tokio::test]
async fn duplicate_directory_paths_rejected_at_startup() {
    let wrapper_def = files_wrapper_paths(
        "/usr/bin/true",
        "gws/state",
        &[".config/gws", ".config/gws"],
    );

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("overlapping files paths"));
}

#[tokio::test]
async fn absolute_directory_path_rejected_at_startup() {
    let wrapper_def = files_wrapper("/usr/bin/true", "gws/state", "/etc/gws");

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("invalid path") || err.contains("path must be relative"));
}

#[tokio::test]
async fn traversal_directory_path_rejected_at_startup() {
    let wrapper_def = files_wrapper("/usr/bin/true", "gws/state", ".config/../gws");

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("invalid path") || err.contains("path must be clean"));
}

#[tokio::test]
async fn empty_segment_directory_path_rejected_at_startup() {
    let wrapper_def = files_wrapper("/usr/bin/true", "gws/state", ".config//gws");

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("invalid path") || err.contains("empty segments"));
}

#[tokio::test]
async fn directory_binding_is_returned_on_read() {
    let mut wrappers = HashMap::new();
    let wrapper_def = files_wrapper_paths(
        "/usr/bin/true",
        "gws/state",
        &[".config/gws", ".local/share/gws"],
    );
    wrappers.insert("dir-w".to_string(), wrapper_def);

    let daemon = TestDaemon::start(wrappers).await.unwrap();
    // Daemon registration seeds a `Locked` permission; unlock it so
    // the read flow can be exercised against the empty resource.
    daemon
        .upsert_permission("test", &daemon.current_permission("dir-w", "gws/state"))
        .await
        .unwrap();
    let resp = daemon.read_request("dir-w").await.unwrap();
    match resp {
        WrapperResponse::ReadOk {
            env,
            resource_id,
            files_target_paths,
            libsecret,
            ..
        } => {
            assert!(env.is_empty());
            assert_eq!(resource_id, "gws/state");
            assert!(!libsecret);
            assert_eq!(
                files_target_paths,
                vec![".config/gws".to_string(), ".local/share/gws".to_string()],
            );
        }
        other => panic!("expected ReadOk, got {other:?}"),
    }
    daemon.stop().await.unwrap();
}

// ===========================================================================
// Concurrent reads
// ===========================================================================
