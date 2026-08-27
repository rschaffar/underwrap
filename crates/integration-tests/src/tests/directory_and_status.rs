use super::*;

#[tokio::test]
async fn overlapping_directory_paths_rejected_at_startup() {
    let wrapper_def = files_wrapper_paths(
        "/usr/bin/true",
        "gws/state",
        &[".config/gws", ".config/gws/session"],
    );

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("files.paths must be unique and non-overlapping"));
}

#[tokio::test]
async fn duplicate_directory_paths_rejected_at_startup() {
    let wrapper_def = files_wrapper_paths(
        "/usr/bin/true",
        "gws/state",
        &[".config/gws", ".config/gws"],
    );

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("files.paths must be unique and non-overlapping"));
}

#[tokio::test]
async fn absolute_directory_path_rejected_at_startup() {
    let wrapper_def = files_wrapper("/usr/bin/true", "gws/state", "/etc/gws");

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("files.paths entries must be canonical relative paths"));
}

#[tokio::test]
async fn traversal_directory_path_rejected_at_startup() {
    let wrapper_def = files_wrapper("/usr/bin/true", "gws/state", ".config/../gws");

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("files.paths entries must be canonical relative paths"));
}

#[tokio::test]
async fn empty_segment_directory_path_rejected_at_startup() {
    let wrapper_def = files_wrapper("/usr/bin/true", "gws/state", ".config//gws");

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("files.paths entries must be canonical relative paths"));
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
    let (resp, blob, _) = checkout_resource(daemon.instance_socket.clone(), "dir-w").await;
    assert_eq!(blob, ResourceBlob::empty());
    match resp {
        WrapperResponse::ReadOk {
            env_projection,
            resource_id,
            files_target_paths,
            libsecret,
            ..
        } => {
            assert!(!env_projection);
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
