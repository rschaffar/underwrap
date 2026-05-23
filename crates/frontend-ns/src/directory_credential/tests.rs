use super::*;

#[test]
fn validate_relative_path_rejects_bad_inputs() {
    for path in ["", "/abs", "a//b", "a/./b", "a/../b", "a\0b"] {
        assert!(
            validate_relative_path(path).is_err(),
            "path {path:?} should fail"
        );
    }
    assert!(validate_relative_path(".config/gws").is_ok());
}

#[test]
fn collect_staging_files_round_trips_files_section() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let mount_a = tempdir.path().join("dir-0/mount-0");
    let mount_b = tempdir.path().join("dir-0/mount-1");
    std::fs::create_dir_all(&mount_a).unwrap();
    std::fs::create_dir_all(mount_a.join("sub")).unwrap();
    std::fs::create_dir_all(&mount_b).unwrap();
    std::fs::write(mount_a.join("a.txt"), b"alpha").unwrap();
    std::fs::write(mount_a.join("sub").join("b.bin"), b"\x00\x01\x02").unwrap();
    std::fs::write(mount_b.join("c.txt"), b"charlie").unwrap();

    let directory = MountedDirectory {
        mounts: vec![
            DirectoryMount {
                target_mount_path: PathBuf::new(),
                created_mount_dirs: Vec::new(),
                staging_path: mount_a,
                mount_relative_path: ".config/gh".to_string(),
            },
            DirectoryMount {
                target_mount_path: PathBuf::new(),
                created_mount_dirs: Vec::new(),
                staging_path: mount_b,
                mount_relative_path: ".config/git".to_string(),
            },
        ],
    };
    let files = collect_staging_files(&directory).unwrap();
    assert_eq!(files.len(), 3);
    assert_eq!(files.get(".config/gh/a.txt").unwrap().as_slice(), b"alpha");
    assert_eq!(
        files.get(".config/gh/sub/b.bin").unwrap().as_slice(),
        b"\x00\x01\x02"
    );
    assert_eq!(
        files.get(".config/git/c.txt").unwrap().as_slice(),
        b"charlie"
    );
}

#[test]
fn route_key_to_mount_picks_matching_prefix() {
    let mounts = vec![".config/gh".to_string(), ".config/git".to_string()];
    assert_eq!(
        route_key_to_mount(".config/gh/hosts.yml", &mounts, "test").unwrap(),
        0
    );
    assert_eq!(
        route_key_to_mount(".config/git/config", &mounts, "test").unwrap(),
        1
    );
    assert_eq!(
        strip_mount_prefix(".config/git/sub/x", ".config/git"),
        "sub/x"
    );
}

#[test]
fn route_key_to_mount_rejects_unknown_prefix() {
    let mounts = vec![".config/gh".to_string()];
    let err = route_key_to_mount(".config/git/config", &mounts, "test").unwrap_err();
    assert!(
        err.contains("no configured mount path is a prefix"),
        "unexpected error: {err}"
    );
}

#[test]
fn route_key_to_mount_rejects_sibling_with_shared_string_prefix() {
    // `.config/ghext` starts with the string `.config/gh` but is not a
    // path-component prefix, so it must not route to the `.config/gh`
    // mount.
    let mounts = vec![".config/gh".to_string()];
    assert!(route_key_to_mount(".config/ghext/x", &mounts, "test").is_err());
}

#[test]
fn route_key_to_mount_rejects_key_equal_to_mount() {
    let mounts = vec![".config/gh".to_string()];
    let err = route_key_to_mount(".config/gh", &mounts, "test").unwrap_err();
    assert!(err.contains("equals mount path"), "unexpected error: {err}");
}
