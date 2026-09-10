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
    let files = collect_staging_files(&plans).unwrap();
    assert_round_trip(&files);
}

#[test]
fn collect_staging_files_refuses_symlinks() {
    // A symlink in the staging aborts the writeback so it can never reach the
    // backend (for an isolated target the walk also runs under setfsuid(X),
    // which bounds any followed path to the target's own uid). Here we assert the
    // static refusal.
    let tempdir = tempfile::TempDir::new().unwrap();
    let staging = tempdir.path().join("mount-0");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("ok.txt"), b"ok").unwrap();
    std::os::unix::fs::symlink("/etc/passwd", staging.join("evil")).unwrap();
    let plans = vec![DirectoryMountPlan {
        mount_path: ".config/gh".to_string(),
        staging_path: staging,
    }];
    let err = collect_staging_files(&plans).unwrap_err();
    assert!(
        err.contains("symlink"),
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

fn blob_with_files(entries: &[(&str, &[u8])]) -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    for (key, value) in entries {
        blob.files
            .insert((*key).to_string(), SecretBytes::from((*value).to_vec()));
    }
    blob
}

fn plan_and_realize_for_test(
    blob: ResourceBlob,
    mount: &str,
    staging_root: &Path,
    target_home: &Path,
) -> Result<(), String> {
    let plan = plan_directory_projection(
        &[mount.to_string()],
        staging_root,
        "complete-projection-test",
        blob,
    )?;
    realize_directory_mounts(target_home, &plan)
}

#[test]
fn complete_projection_later_unroutable_has_zero_staging_side_effects() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let staging_root = tempdir.path().join("staging");
    let target_home = tempdir.path().join("home");
    std::fs::create_dir(&target_home).unwrap();
    let blob = blob_with_files(&[
        (".config/gate/a-preserved", b"first"),
        (".config/z-gate/missing", b"later"),
    ]);

    let error =
        plan_and_realize_for_test(blob, ".config/gate", &staging_root, &target_home).unwrap_err();
    assert!(
        error.contains("no configured mount path is a prefix"),
        "unexpected error: {error}"
    );
    assert!(!staging_root.exists());
    assert_eq!(std::fs::read_dir(&target_home).unwrap().count(), 0);
}

#[test]
fn complete_projection_file_descendant_collision_has_zero_staging_side_effects() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let staging_root = tempdir.path().join("staging");
    let target_home = tempdir.path().join("home");
    std::fs::create_dir(&target_home).unwrap();
    let blob = blob_with_files(&[
        (".config/gate/a", b"file"),
        (".config/gate/a/b", b"descendant"),
    ]);

    let error =
        plan_and_realize_for_test(blob, ".config/gate", &staging_root, &target_home).unwrap_err();
    assert!(
        error.contains("colliding file keys"),
        "unexpected error: {error}"
    );
    assert!(error.contains(".config/gate/a"));
    assert!(error.contains(".config/gate/a/b"));
    assert!(!staging_root.exists());
    assert_eq!(std::fs::read_dir(&target_home).unwrap().count(), 0);
}

#[test]
fn complete_projection_valid_input_routes_every_file_exactly_once() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let blob = blob_with_files(&[
        (".config/gh/hosts.yml", b"gh"),
        (".config/git/config", b"git"),
    ]);
    let plan = plan_directory_projection(
        &[".config/gh".to_string(), ".config/git".to_string()],
        tempdir.path(),
        "complete-projection-test",
        blob,
    )
    .unwrap();

    assert_eq!(plan.mounts.len(), 2);
    assert_eq!(plan.files.len(), 2);
    assert_eq!(plan.files[0].mount_index, 0);
    assert_eq!(plan.files[0].relative_path, "hosts.yml");
    assert_eq!(plan.files[1].mount_index, 1);
    assert_eq!(plan.files[1].relative_path, "config");
}

#[test]
fn complete_projection_rejects_multiple_strict_routes() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let blob = blob_with_files(&[("a/b/c", b"value")]);
    let error = plan_directory_projection(
        &["a".to_string(), "a/b".to_string()],
        tempdir.path(),
        "complete-projection-test",
        blob,
    )
    .unwrap_err();
    assert!(error.contains("multiple configured mount paths"));
}
