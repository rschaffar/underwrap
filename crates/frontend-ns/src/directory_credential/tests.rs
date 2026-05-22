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
    let staging = tempdir.path().join("dir-0");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(staging.join("sub")).unwrap();
    std::fs::write(staging.join("a.txt"), b"alpha").unwrap();
    std::fs::write(staging.join("sub").join("b.bin"), b"\x00\x01\x02").unwrap();

    let directory = MountedDirectory {
        resource_id: "test".to_string(),
        mounts: Vec::new(),
        staging_path: staging.clone(),
    };
    let files = collect_staging_files(&directory).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files.get("a.txt").unwrap().as_slice(), b"alpha");
    assert_eq!(files.get("sub/b.bin").unwrap().as_slice(), b"\x00\x01\x02");
}
