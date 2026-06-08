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

fn round_trip_plans(tempdir: &Path) -> Vec<DirectoryMountPlan> {
    let staging_a = tempdir.join("mount-0");
    let staging_b = tempdir.join("mount-1");
    std::fs::create_dir_all(&staging_a).unwrap();
    std::fs::create_dir_all(staging_a.join("sub")).unwrap();
    std::fs::create_dir_all(&staging_b).unwrap();
    std::fs::write(staging_a.join("a.txt"), b"alpha").unwrap();
    std::fs::write(staging_a.join("sub").join("b.bin"), b"\x00\x01\x02").unwrap();
    std::fs::write(staging_b.join("c.txt"), b"charlie").unwrap();
    vec![
        DirectoryMountPlan {
            mount_path: ".config/gh".to_string(),
            staging_path: staging_a,
        },
        DirectoryMountPlan {
            mount_path: ".config/git".to_string(),
            staging_path: staging_b,
        },
    ]
}

fn assert_round_trip(files: &std::collections::BTreeMap<String, SecretBytes>) {
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
fn collect_staging_files_round_trips_files_section() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let plans = round_trip_plans(tempdir.path());
    let files = collect_staging_files(&plans, false).unwrap();
    assert_round_trip(&files);
}

/// True when `openat2` is available on the test host (Linux 5.6+). The hardened
/// walk relies on it; skip those tests on older kernels rather than fail.
fn openat2_available() -> bool {
    let root = std::ffi::CString::new("/").unwrap();
    match openat2(
        libc::AT_FDCWD,
        &root,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        RESOLVE_NO_SYMLINKS,
    ) {
        Ok(_) => true,
        Err(e) => e.raw_os_error() != Some(libc::ENOSYS),
    }
}

#[test]
fn collect_staging_files_hardened_round_trips_files_section() {
    if !openat2_available() {
        eprintln!("skipping: openat2 unavailable on this kernel");
        return;
    }
    let tempdir = tempfile::TempDir::new().unwrap();
    let plans = round_trip_plans(tempdir.path());
    let files = collect_staging_files(&plans, true).unwrap();
    assert_round_trip(&files);
}

#[test]
fn collect_staging_files_hardened_refuses_symlinks() {
    if !openat2_available() {
        eprintln!("skipping: openat2 unavailable on this kernel");
        return;
    }
    let tempdir = tempfile::TempDir::new().unwrap();
    let staging = tempdir.path().join("mount-0");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("ok.txt"), b"ok").unwrap();
    std::os::unix::fs::symlink("/etc/passwd", staging.join("evil")).unwrap();
    let plans = vec![DirectoryMountPlan {
        mount_path: ".config/gh".to_string(),
        staging_path: staging,
    }];
    let err = collect_staging_files(&plans, true).unwrap_err();
    assert!(
        err.contains("symlinks") || err.contains("evil"),
        "expected a symlink-refusal error, got: {err}"
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
