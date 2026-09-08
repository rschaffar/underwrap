use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::{is_exact_private_mode, require_directory};

#[test]
fn private_mode_predicate_rejects_permission_and_special_bits() {
    for mode in [0o755, 0o1700, 0o2700, 0o4700] {
        assert!(!is_exact_private_mode(mode), "mode {mode:o}");
    }
    assert!(is_exact_private_mode(0o0700));
}

#[test]
fn private_directory_requires_exact_0700_on_the_filesystem() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("workspace");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o0755)).unwrap();
    assert!(require_directory(&dir, true).is_err());
    assert!(require_directory(&dir, false).is_ok());

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o0700)).unwrap();
    assert!(require_directory(&dir, true).is_ok());
}
