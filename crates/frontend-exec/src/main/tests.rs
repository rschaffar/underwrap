use super::*;
use underwrap_protocol::target_env::{FORCED_TARGET_ENV_NAMES, CONFIGURABLE_FORCED_TARGET_ENV_NAMES};

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

#[test]
fn parse_wrapper_config_rejects_forced_passthrough_names() {
    for name in FORCED_TARGET_ENV_NAMES {
        let yaml = format!(
            r#"
 target: /usr/bin/true
 wrapper_socket: /run/underwrap-local/wrapper.sock
 xdg_runtime_dir_base: /run/user
 env_passthrough:
   - {name}
 "#
        );
        let err = parse_wrapper_config_text(&yaml).unwrap_err().to_string();
        assert!(
            err.contains(name),
            "expected error to mention {name}, got: {err}"
        );
    }
}

#[test]
fn parse_wrapper_config_rejects_xdg_runtime_dir_in_env_set() {
    let yaml = r#"
 target: /usr/bin/true
 wrapper_socket: /run/underwrap-local/wrapper.sock
 xdg_runtime_dir_base: /run/user
 env_passthrough:
   - SSH_AUTH_SOCK
 env_set:
   PATH: /trusted/bin
   XDG_RUNTIME_DIR: /tmp/evil
 "#;
    let err = parse_wrapper_config_text(yaml).unwrap_err().to_string();
    assert!(err.contains("XDG_RUNTIME_DIR"));
    assert!(err.contains("xdg_runtime_dir_base"));
}

#[test]
fn parse_wrapper_config_rejects_non_absolute_xdg_runtime_dir_base() {
    let yaml = r#"
 target: /usr/bin/true
 wrapper_socket: /run/underwrap-local/wrapper.sock
 xdg_runtime_dir_base: relative/runtime
 "#;
    let err = parse_wrapper_config_text(yaml).unwrap_err().to_string();
    assert!(err.contains("xdg_runtime_dir_base must be an absolute path"));
}

#[test]
fn parse_wrapper_config_rejects_non_absolute_home_and_base_xdg_env_set_paths() {
    for name in CONFIGURABLE_FORCED_TARGET_ENV_NAMES {
        let yaml = format!(
            r#"
 target: /usr/bin/true
 wrapper_socket: /run/underwrap-local/wrapper.sock
 xdg_runtime_dir_base: /run/user
 env_set:
   {name}: relative/path
 "#
        );
        let err = parse_wrapper_config_text(&yaml).unwrap_err().to_string();
        assert!(err.contains(&format!("env_set {name} must be an absolute path")));
    }
}

#[test]
fn parse_wrapper_config_accepts_default_and_normal_passthrough() {
    let yaml = r#"
 target: /usr/bin/true
 wrapper_socket: /run/underwrap-local/wrapper.sock
 env_passthrough:
   - SSH_AUTH_SOCK
 env_set:
   PATH: /trusted/bin
 "#;
    let config = parse_wrapper_config_text(yaml).unwrap();
    assert_eq!(config.xdg_runtime_dir_base, "/run/user");
    assert_eq!(config.env_passthrough, vec!["SSH_AUTH_SOCK".to_string()]);
    assert_eq!(
        config.env_set.get("PATH"),
        Some(&"/trusted/bin".to_string())
    );
}

#[test]
fn parse_wrapper_config_accepts_custom_absolute_xdg_runtime_dir_base() {
    let yaml = r#"
 target: /usr/bin/true
 wrapper_socket: /run/underwrap-local/wrapper.sock
 xdg_runtime_dir_base: /run/underwrap-runtime
 "#;
    let config = parse_wrapper_config_text(yaml).unwrap();
    assert_eq!(config.xdg_runtime_dir_base, "/run/underwrap-runtime");
}
