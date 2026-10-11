use super::*;

#[test]
fn collection_keeps_original_objects_after_staging_ancestor_replacement() {
    let temp = tempfile::TempDir::new().unwrap();
    let staging = temp.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    let mounts = round_trip_plans(&staging);

    std::fs::rename(&staging, temp.path().join("moved")).unwrap();
    let replacement = round_trip_plans(&staging);
    std::fs::write(staging.join("mount-0/a.txt"), b"replacement").unwrap();

    let original_files = collect_staging_files(&mounts).unwrap();
    assert_round_trip(&original_files);
    let replacement_files = collect_staging_files(&replacement).unwrap();
    assert!(replacement_files[".config/gh/a.txt"].as_slice() == b"replacement");
    // A second walk must not share a consumed directory-stream position.
    assert_round_trip(&collect_staging_files(&mounts).unwrap());
}

#[test]
fn materialization_keeps_original_objects_after_early_ancestor_replacement() {
    let temp = tempfile::TempDir::new().unwrap();
    let staging = temp.path().join("staging");
    std::fs::create_dir(&staging).unwrap();
    let root = Directory::open_path(&staging).unwrap();
    let plan = plan_directory_projection(
        &[".config/tool".into()],
        "replacement-test",
        blob_with_files(&[(".config/tool/sub/state", b"original")]),
    )
    .unwrap();
    let mounts = prepare_mount_directories(&plan, &root).unwrap();

    std::fs::rename(&staging, temp.path().join("moved")).unwrap();
    std::fs::create_dir_all(staging.join("mount-0/sub")).unwrap();
    std::fs::write(staging.join("mount-0/sub/state"), b"replacement").unwrap();

    materialize_directory_files(&plan, &mounts).unwrap();
    assert!(std::fs::read(temp.path().join("moved/mount-0/sub/state")).unwrap() == b"original");
    assert!(std::fs::read(staging.join("mount-0/sub/state")).unwrap() == b"replacement");
    let files = collect_staging_files(&mounts).unwrap();
    assert!(files[".config/tool/sub/state"].as_slice() == b"original");
}

#[test]
fn replacing_projection_directory_name_does_not_redirect_collection() {
    let temp = tempfile::TempDir::new().unwrap();
    let mounts = round_trip_plans(temp.path());
    std::fs::rename(temp.path().join("mount-0"), temp.path().join("old-mount")).unwrap();
    std::fs::create_dir(temp.path().join("mount-0")).unwrap();
    std::fs::write(temp.path().join("mount-0/a.txt"), b"replacement").unwrap();
    assert_round_trip(&collect_staging_files(&mounts).unwrap());
}

#[test]
fn collection_refuses_directory_symlinks_and_special_files() {
    let temp = tempfile::TempDir::new().unwrap();
    let mounts = round_trip_plans(temp.path());
    let symlink = temp.path().join("mount-0/outside");
    std::os::unix::fs::symlink(temp.path().join("mount-1"), &symlink).unwrap();
    assert!(
        collect_staging_files(&mounts)
            .unwrap_err()
            .contains("symlink")
    );
    std::fs::remove_file(symlink).unwrap();

    use std::os::unix::ffi::OsStrExt;
    let fifo =
        std::ffi::CString::new(temp.path().join("mount-0/fifo").as_os_str().as_bytes()).unwrap();
    // SAFETY: a live NUL-terminated synthetic fixture path; result is checked.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert!(
        collect_staging_files(&mounts)
            .unwrap_err()
            .contains("unsupported entry type")
    );
}

#[test]
fn materialization_rejects_symlink_parent_without_writing_outside() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = Directory::open_path(temp.path()).unwrap();
    let outside = tempfile::TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), temp.path().join("sub")).unwrap();
    assert!(materialize_file(&root, "sub/state", b"synthetic").is_err());
    assert!(!outside.path().join("state").exists());
}

#[test]
fn mismatched_projection_handles_fail_before_materialization() {
    let temp = tempfile::TempDir::new().unwrap();
    let root = Directory::open_path(temp.path()).unwrap();
    let plan = plan_directory_projection(
        &[".config/tool".into()],
        "replacement-test",
        blob_with_files(&[(".config/tool/state", b"synthetic")]),
    )
    .unwrap();
    let mut mounts = prepare_mount_directories(&plan, &root).unwrap();
    mounts[0].mount_path = ".config/other".into();
    assert!(materialize_directory_files(&plan, &mounts).is_err());
    assert!(!temp.path().join("mount-0/state").exists());
}
