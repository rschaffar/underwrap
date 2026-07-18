use super::*;
use std::collections::HashMap;
use underwrap_protocol::config::AccessMode;
use underwrap_protocol::exec::ExecResourceBinding;
use underwrap_protocol::target_env::{
    CONFIGURABLE_FORCED_TARGET_ENV_NAMES, FORCED_TARGET_ENV_NAMES,
};

fn sample_binding() -> ExecResourceBinding {
    ExecResourceBinding {
        resource_id: "res_x_y".to_string(),
        resource_name: "x/y".to_string(),
        access: AccessMode::ReadOnly,
        libsecret: false,
        files_target_paths: Vec::new(),
    }
}

fn sample_exec_bundle() -> ExecBundle {
    ExecBundle {
        env: HashMap::from([("GH_TOKEN".to_string(), "ghp_test".into())]),
        env_passthrough: vec!["CUSTOM_PASSTHROUGH".to_string()],
        env_set: HashMap::from([("PATH".to_string(), "/trusted/bin".to_string())]),
        target_runtime_dir_base: "/run/underwrap-runtime".to_string(),
        resource: sample_binding(),
        access_paths: Vec::new(),
    }
}

#[test]
fn build_target_env_forces_home_and_xdg_from_trusted_inputs() {
    let host_env = HashMap::from([
        ("HOME".to_string(), "/tmp/evil-home".to_string()),
        ("XDG_RUNTIME_DIR".to_string(), "/tmp/evil-run".to_string()),
        (
            "XDG_CONFIG_HOME".to_string(),
            "/tmp/evil-config".to_string(),
        ),
        ("XDG_DATA_HOME".to_string(), "/tmp/evil-data".to_string()),
        ("XDG_CACHE_HOME".to_string(), "/tmp/evil-cache".to_string()),
        ("XDG_STATE_HOME".to_string(), "/tmp/evil-state".to_string()),
        ("LANG".to_string(), "en_US.UTF-8".to_string()),
        ("CUSTOM_PASSTHROUGH".to_string(), "allowed".to_string()),
    ]);

    let target_env = build_target_env(
        &host_env,
        &sample_exec_bundle(),
        std::path::Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap();

    assert_eq!(target_env.get("HOME"), Some(&"/home/alice".to_string()));
    assert_eq!(
        target_env.get("XDG_RUNTIME_DIR"),
        Some(&"/run/underwrap-runtime/underwrap-target-abc".to_string())
    );
    assert_eq!(
        target_env.get("XDG_CONFIG_HOME"),
        Some(&"/home/alice/.config".to_string())
    );
    assert_eq!(
        target_env.get("XDG_DATA_HOME"),
        Some(&"/home/alice/.local/share".to_string())
    );
    assert_eq!(
        target_env.get("XDG_CACHE_HOME"),
        Some(&"/home/alice/.cache".to_string())
    );
    assert_eq!(
        target_env.get("XDG_STATE_HOME"),
        Some(&"/home/alice/.local/state".to_string())
    );
    assert_eq!(target_env.get("LANG"), Some(&"en_US.UTF-8".to_string()));
    assert_eq!(
        target_env.get("CUSTOM_PASSTHROUGH"),
        Some(&"allowed".to_string())
    );
    assert_eq!(target_env.get("PATH"), Some(&"/trusted/bin".to_string()));
    assert_eq!(target_env.get("GH_TOKEN"), Some(&"ghp_test".to_string()));
}

#[test]
fn build_target_env_derives_base_xdg_dirs_from_env_set_home() {
    let bundle = ExecBundle {
        env: HashMap::new(),
        env_passthrough: Vec::new(),
        env_set: HashMap::from([("HOME".to_string(), "/srv/wrapped-home".to_string())]),
        target_runtime_dir_base: "/run/user".to_string(),
        resource: sample_binding(),
        access_paths: Vec::new(),
    };

    let target_env = build_target_env(
        &HashMap::new(),
        &bundle,
        std::path::Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap();

    assert_eq!(
        target_env.get("HOME"),
        Some(&"/srv/wrapped-home".to_string())
    );
    assert_eq!(
        target_env.get("XDG_CONFIG_HOME"),
        Some(&"/srv/wrapped-home/.config".to_string())
    );
    assert_eq!(
        target_env.get("XDG_DATA_HOME"),
        Some(&"/srv/wrapped-home/.local/share".to_string())
    );
    assert_eq!(
        target_env.get("XDG_CACHE_HOME"),
        Some(&"/srv/wrapped-home/.cache".to_string())
    );
    assert_eq!(
        target_env.get("XDG_STATE_HOME"),
        Some(&"/srv/wrapped-home/.local/state".to_string())
    );
    assert_eq!(
        target_env.get("XDG_RUNTIME_DIR"),
        Some(&"/run/underwrap-runtime/underwrap-target-abc".to_string())
    );
}

#[test]
fn build_target_env_honors_explicit_base_xdg_overrides() {
    let bundle = ExecBundle {
        env: HashMap::new(),
        env_passthrough: vec!["XDG_CONFIG_HOME".to_string()],
        env_set: HashMap::from([
            ("HOME".to_string(), "/srv/wrapped-home".to_string()),
            (
                "XDG_CONFIG_HOME".to_string(),
                "/srv/custom-config".to_string(),
            ),
            (
                "XDG_CACHE_HOME".to_string(),
                "/srv/custom-cache".to_string(),
            ),
            (
                "XDG_STATE_HOME".to_string(),
                "/srv/custom-state".to_string(),
            ),
        ]),
        target_runtime_dir_base: "/run/user".to_string(),
        resource: sample_binding(),
        access_paths: Vec::new(),
    };
    let host_env = HashMap::from([(
        "XDG_CONFIG_HOME".to_string(),
        "/tmp/evil-config".to_string(),
    )]);

    let target_env = build_target_env(
        &host_env,
        &bundle,
        std::path::Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap();

    assert_eq!(
        target_env.get("XDG_CONFIG_HOME"),
        Some(&"/srv/custom-config".to_string())
    );
    assert_eq!(
        target_env.get("XDG_CACHE_HOME"),
        Some(&"/srv/custom-cache".to_string())
    );
    assert_eq!(
        target_env.get("XDG_STATE_HOME"),
        Some(&"/srv/custom-state".to_string())
    );
    assert_eq!(
        target_env.get("XDG_DATA_HOME"),
        Some(&"/srv/wrapped-home/.local/share".to_string())
    );
}

#[test]
fn build_target_env_ignores_forced_names_in_passthrough_and_env_set() {
    let bundle = ExecBundle {
        env: HashMap::new(),
        env_passthrough: FORCED_TARGET_ENV_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        env_set: HashMap::from([(
            "XDG_RUNTIME_DIR".to_string(),
            "/tmp/evil-runtime".to_string(),
        )]),
        target_runtime_dir_base: "/run/secure-runtime".to_string(),
        resource: sample_binding(),
        access_paths: Vec::new(),
    };
    let host_env = HashMap::from([
        ("HOME".to_string(), "/tmp/evil-home".to_string()),
        (
            "XDG_RUNTIME_DIR".to_string(),
            "/tmp/evil-runtime".to_string(),
        ),
        (
            "XDG_CONFIG_HOME".to_string(),
            "/tmp/evil-config".to_string(),
        ),
    ]);

    let target_env = build_target_env(
        &host_env,
        &bundle,
        std::path::Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap();

    assert_eq!(target_env.get("HOME"), Some(&"/home/alice".to_string()));
    // The env_set XDG_RUNTIME_DIR override is ignored; the resolved value wins.
    assert_eq!(
        target_env.get("XDG_RUNTIME_DIR"),
        Some(&"/run/underwrap-runtime/underwrap-target-abc".to_string())
    );
    assert_eq!(
        target_env.get("XDG_CONFIG_HOME"),
        Some(&"/home/alice/.config".to_string())
    );
}

#[test]
fn build_target_env_rejects_relative_home_and_base_xdg_overrides() {
    for name in CONFIGURABLE_FORCED_TARGET_ENV_NAMES {
        let bundle = ExecBundle {
            env: HashMap::new(),
            env_passthrough: Vec::new(),
            env_set: HashMap::from([(name.to_string(), "relative/path".to_string())]),
            target_runtime_dir_base: "/run/user".to_string(),
            resource: sample_binding(),
            access_paths: Vec::new(),
        };
        let err = build_target_env(
            &HashMap::new(),
            &bundle,
            std::path::Path::new("/home/alice"),
            "/run/underwrap-runtime/underwrap-target-abc",
        )
        .unwrap_err();
        assert!(err.contains(&format!("{name} must be an absolute path")));
    }
}

#[test]
fn build_target_env_rejects_non_utf8_secret_values() {
    let host_env = HashMap::new();
    let bundle = ExecBundle {
        env: HashMap::from([("BAD".to_string(), vec![0xff, 0xfe].into())]),
        env_passthrough: Vec::new(),
        env_set: HashMap::new(),
        target_runtime_dir_base: "/run/user".to_string(),
        resource: sample_binding(),
        access_paths: Vec::new(),
    };

    let err = build_target_env(
        &host_env,
        &bundle,
        std::path::Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap_err();
    assert!(err.contains("not valid UTF-8"));
    assert!(err.contains("BAD"));
}
