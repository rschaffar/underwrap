use super::*;

#[tokio::test]
async fn overlapping_directory_paths_rejected_at_startup() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let wrapper_socket = base.join("wrapper.sock");

    let mut wrapper_def = wrapper("/usr/bin/true", "dummy/env", "IGNORED");
    wrapper_def.resources = HashMap::from([
        ("gws/state".to_string(), files_resource(".config/gws")),
        (
            "gws/state-nested".to_string(),
            files_resource(".config/gws/session"),
        ),
    ]);
    wrapper_def.socket = wrapper_socket;

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("overlapping directory resource paths"));
}

#[tokio::test]
async fn duplicate_directory_paths_rejected_at_startup() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let wrapper_socket = base.join("wrapper.sock");

    let mut wrapper_def = wrapper("/usr/bin/true", "dummy/env", "IGNORED");
    wrapper_def.resources = HashMap::from([
        ("gws/state-a".to_string(), files_resource(".config/gws")),
        ("gws/state-b".to_string(), files_resource(".config/gws")),
    ]);
    wrapper_def.socket = wrapper_socket;

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("overlapping directory resource paths"));
}

#[tokio::test]
async fn absolute_directory_path_rejected_at_startup() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let wrapper_socket = base.join("wrapper.sock");

    let mut wrapper_def = wrapper("/usr/bin/true", "dummy/env", "IGNORED");
    wrapper_def.resources = HashMap::from([("gws/state".to_string(), files_resource("/etc/gws"))]);
    wrapper_def.socket = wrapper_socket;

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("invalid path") || err.contains("path must be relative"));
}

#[tokio::test]
async fn traversal_directory_path_rejected_at_startup() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let wrapper_socket = base.join("wrapper.sock");

    let mut wrapper_def = wrapper("/usr/bin/true", "dummy/env", "IGNORED");
    wrapper_def.resources =
        HashMap::from([("gws/state".to_string(), files_resource(".config/../gws"))]);
    wrapper_def.socket = wrapper_socket;

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("invalid path") || err.contains("path must be clean"));
}

#[tokio::test]
async fn empty_segment_directory_path_rejected_at_startup() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let base = tempdir.path();
    let wrapper_socket = base.join("wrapper.sock");

    let mut wrapper_def = wrapper("/usr/bin/true", "dummy/env", "IGNORED");
    wrapper_def.resources =
        HashMap::from([("gws/state".to_string(), files_resource(".config//gws"))]);
    wrapper_def.socket = wrapper_socket;

    let err = daemon_config_error(wrapper_def).await;
    assert!(err.contains("invalid path") || err.contains("empty segments"));
}

#[tokio::test]
async fn directory_bindings_are_returned_on_read() {
    let mut wrappers = HashMap::new();
    let mut wrapper_def = wrapper("/usr/bin/true", "dummy/env", "IGNORED");
    wrapper_def.resources = HashMap::from([
        ("gws/state-a".to_string(), files_resource(".config/gws-a")),
        (
            "gws/state-b".to_string(),
            files_resource(".local/share/gws-b"),
        ),
    ]);
    wrappers.insert("dir-w".to_string(), wrapper_def);

    let daemon = TestDaemon::start(wrappers).await.unwrap();
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
