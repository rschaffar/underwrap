use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::require_directory;

#[test]
fn private_directory_requires_exact_0700_including_special_bits() {
    let tmp = tempfile::TempDir::new().unwrap();

    for mode in [0o755, 0o1700, 0o2700, 0o4700] {
        let dir = tmp.path().join(format!("workspace-{mode:o}"));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(mode)).unwrap();

        assert!(require_directory(&dir, true).is_err(), "mode {mode:o}");
        assert!(require_directory(&dir, false).is_ok(), "mode {mode:o}");
    }

    let exact = tmp.path().join("workspace-0700");
    fs::create_dir(&exact).unwrap();
    fs::set_permissions(&exact, fs::Permissions::from_mode(0o0700)).unwrap();
    assert!(require_directory(&exact, true).is_ok());
}
