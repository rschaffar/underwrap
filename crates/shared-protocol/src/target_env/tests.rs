use super::*;

#[test]
fn forced_target_env_names_cover_every_special_case() {
    assert!(is_forced_target_env_name("HOME"));
    assert!(is_forced_target_env_name("XDG_RUNTIME_DIR"));
    assert!(is_forced_target_env_name("XDG_CONFIG_HOME"));
    assert!(is_forced_target_env_name("XDG_DATA_HOME"));
    assert!(is_forced_target_env_name("XDG_CACHE_HOME"));
    assert!(is_forced_target_env_name("XDG_STATE_HOME"));
    assert!(!is_forced_target_env_name("LANG"));
}

#[test]
fn only_home_and_base_xdg_dirs_are_env_set_overridable() {
    assert!(is_home_or_base_xdg_dir_env_name("HOME"));
    assert!(is_home_or_base_xdg_dir_env_name("XDG_CONFIG_HOME"));
    assert!(is_home_or_base_xdg_dir_env_name("XDG_DATA_HOME"));
    assert!(is_home_or_base_xdg_dir_env_name("XDG_CACHE_HOME"));
    assert!(is_home_or_base_xdg_dir_env_name("XDG_STATE_HOME"));
    assert!(!is_home_or_base_xdg_dir_env_name("XDG_RUNTIME_DIR"));
    assert!(!is_home_or_base_xdg_dir_env_name("PATH"));
}
