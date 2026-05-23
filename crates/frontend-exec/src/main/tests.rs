use super::config_name_from_exe_path;

#[test]
fn config_name_uses_executable_basename() {
    let name = config_name_from_exe_path(std::path::Path::new(
        "/run/wrappers/bin/debug-robert-gh-authorized",
    ))
    .unwrap();
    assert_eq!(name, "debug-robert-gh-authorized");
}

#[test]
fn config_name_rejects_missing_basename() {
    let err = config_name_from_exe_path(std::path::Path::new("/"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("no final path component"));
}
