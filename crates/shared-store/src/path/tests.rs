use super::*;

fn temp_storage_dir() -> tempfile::TempDir {
    tempfile::TempDir::new().unwrap()
}

#[test]
fn validated_path_accepts_clean_relative_ids() {
    let dir = temp_storage_dir();
    let base = dir.path();

    assert_eq!(validated_path(base, "token").unwrap(), base.join("token"));
    assert_eq!(
        validated_path(base, "github/work/token").unwrap(),
        base.join("github/work/token")
    );
}

#[test]
fn validated_path_rejects_invalid_ids() {
    let dir = temp_storage_dir();
    let base = dir.path();

    for bad in ["", "/abs", ".", "..", "a//b", "a/./b", "a/../b", "bad\0id"] {
        assert!(
            validated_path(base, bad).is_err(),
            "expected rejection for {bad:?}"
        );
    }
}

#[test]
fn validated_path_rejects_symlinked_parent_escape() {
    use std::os::unix::fs::symlink;

    let dir = temp_storage_dir();
    let base = dir.path();
    let outside = tempfile::TempDir::new().unwrap();

    symlink(outside.path(), base.join("escape")).unwrap();

    let err = validated_path(base, "escape/token").unwrap_err();
    assert!(err.to_string().contains("escapes storage directory"));
}

#[test]
fn list_files_recursive_lists_all_files_sorted_and_relative() {
    let dir = temp_storage_dir();
    let base = dir.path();

    std::fs::create_dir_all(base.join("a/b")).unwrap();
    std::fs::create_dir_all(base.join("a/c")).unwrap();
    std::fs::write(base.join("z.txt"), b"z").unwrap();
    std::fs::write(base.join("a/b/one"), b"1").unwrap();
    std::fs::write(base.join("a/c/two"), b"2").unwrap();

    let ids = list_files_recursive(base, "").unwrap();
    assert_eq!(ids, vec!["a/b/one", "a/c/two", "z.txt"]);
}

#[test]
fn list_files_recursive_missing_prefix_is_empty() {
    let dir = temp_storage_dir();
    let ids = list_files_recursive(dir.path(), "missing/prefix").unwrap();
    assert!(ids.is_empty());
}

#[test]
fn list_files_recursive_rejects_invalid_prefix() {
    let dir = temp_storage_dir();
    let base = dir.path();

    for bad in [".", "..", "a//b", "a/./b", "a/../b"] {
        assert!(
            list_files_recursive(base, bad).is_err(),
            "expected invalid prefix rejection for {bad:?}"
        );
    }
}

#[test]
fn cleanup_empty_parents_removes_empty_ancestors_but_not_base() {
    let dir = temp_storage_dir();
    let base = dir.path();
    let file = base.join("a/b/c/token");

    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"secret").unwrap();
    std::fs::remove_file(&file).unwrap();

    cleanup_empty_parents(&file, base);

    assert!(base.exists());
    assert!(!base.join("a").exists());
}

#[test]
fn cleanup_empty_parents_stops_at_non_empty_parent() {
    let dir = temp_storage_dir();
    let base = dir.path();
    let removed = base.join("a/b/c/token");
    let sibling = base.join("a/b/keep");

    std::fs::create_dir_all(removed.parent().unwrap()).unwrap();
    std::fs::write(&removed, b"secret").unwrap();
    std::fs::write(&sibling, b"keep").unwrap();
    std::fs::remove_file(&removed).unwrap();

    cleanup_empty_parents(&removed, base);

    assert!(base.join("a/b").exists());
    assert!(!base.join("a/b/c").exists());
    assert!(sibling.exists());
}

#[test]
fn prepare_dir_creates_and_canonicalizes_directory() {
    let dir = temp_storage_dir();
    let path = dir.path().join("store/subdir");

    let prepared = prepare_dir(&path).unwrap();

    assert!(path.exists());
    assert_eq!(prepared, path.canonicalize().unwrap());
}
