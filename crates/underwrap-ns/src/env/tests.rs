use super::*;
use std::collections::HashMap;
use underwrap_protocol::access::AccessMode;
use underwrap_protocol::exec::ExecResourceBinding;
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::ResourceBlob;
use underwrap_protocol::target_env::{
    CONFIGURABLE_FORCED_TARGET_ENV_NAMES, FORCED_TARGET_ENV_NAMES,
};

fn sample_binding() -> ExecResourceBinding {
    ExecResourceBinding {
        resource_id: "res_x_y".to_string(),
        resource_name: "x/y".to_string(),
        access: AccessMode::ReadOnly,
        env_projection: true,
        libsecret: false,
        files_target_paths: Vec::new(),
    }
}

fn sample_blob() -> ResourceBlob {
    let mut blob = ResourceBlob::empty();
    blob.env
        .insert("GH_TOKEN".to_string(), SecretBytes::from("ghp_test"));
    blob
}

fn sample_exec_bundle() -> ExecBundle {
    ExecBundle {
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
        &sample_blob(),
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
        env_passthrough: Vec::new(),
        env_set: HashMap::from([("HOME".to_string(), "/srv/wrapped-home".to_string())]),
        target_runtime_dir_base: "/run/user".to_string(),
        resource: sample_binding(),
        access_paths: Vec::new(),
    };

    let target_env = build_target_env(
        &HashMap::new(),
        &bundle,
        &ResourceBlob::empty(),
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
        &ResourceBlob::empty(),
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
        &ResourceBlob::empty(),
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
            env_passthrough: Vec::new(),
            env_set: HashMap::from([(name.to_string(), "relative/path".to_string())]),
            target_runtime_dir_base: "/run/user".to_string(),
            resource: sample_binding(),
            access_paths: Vec::new(),
        };
        let err = build_target_env(
            &HashMap::new(),
            &bundle,
            &ResourceBlob::empty(),
            std::path::Path::new("/home/alice"),
            "/run/underwrap-runtime/underwrap-target-abc",
        )
        .unwrap_err();
        assert!(err.contains(&format!("{name} must be an absolute path")));
    }
}

#[test]
fn build_target_env_rejects_invalid_checkout_names_during_pure_planning() {
    let mut checkout = ResourceBlob::empty();
    checkout
        .env
        .insert("BAD=NAME".to_string(), SecretBytes::from(b"value".to_vec()));
    let err = build_target_env(
        &HashMap::new(),
        &sample_exec_bundle(),
        &checkout,
        std::path::Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap_err();
    assert!(err.contains("BAD=NAME"));
    assert!(err.contains("invalid"));
}

#[test]
fn build_target_env_rejects_non_utf8_secret_values_during_pure_planning() {
    let host_env = HashMap::new();
    let bundle = ExecBundle {
        env_passthrough: Vec::new(),
        env_set: HashMap::new(),
        target_runtime_dir_base: "/run/user".to_string(),
        resource: sample_binding(),
        access_paths: Vec::new(),
    };

    let mut checkout = ResourceBlob::empty();
    checkout
        .env
        .insert("BAD".to_string(), SecretBytes::from(vec![0xff, 0xfe]));
    let err = build_target_env(
        &host_env,
        &bundle,
        &checkout,
        std::path::Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap_err();
    assert!(err.contains("not valid UTF-8"));
    assert!(err.contains("BAD"));
}

#[test]
fn build_target_env_rejects_nul_checkout_values_during_pure_planning() {
    for value in ["\0leading", "interior\0nul", "trailing\0"] {
        let mut checkout = ResourceBlob::empty();
        checkout
            .env
            .insert("CHECKOUT_VALUE".to_string(), SecretBytes::from(value));
        let err = build_target_env(
            &HashMap::new(),
            &sample_exec_bundle(),
            &checkout,
            std::path::Path::new("/home/alice"),
            "/run/underwrap-runtime/underwrap-target-abc",
        )
        .unwrap_err();
        assert!(err.contains("CHECKOUT_VALUE"));
        assert!(err.contains("NUL"));
        assert!(!err.contains(value));
    }
}

#[test]
fn build_target_env_rejects_nul_env_set_value() {
    let mut bundle = sample_exec_bundle();
    bundle
        .env_set
        .insert("OPERATOR_VALUE".to_string(), "operator\0value".to_string());
    let err = build_target_env(
        &HashMap::new(),
        &bundle,
        &ResourceBlob::empty(),
        std::path::Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap_err();
    assert!(err.contains("OPERATOR_VALUE"));
    assert!(err.contains("NUL"));
    assert!(!err.contains("operator"));
}

#[test]
fn build_target_env_rejects_invalid_env_set_key() {
    let mut bundle = sample_exec_bundle();
    bundle
        .env_set
        .insert("INVALID=KEY".to_string(), "trusted".to_string());
    let err = build_target_env(
        &HashMap::new(),
        &bundle,
        &ResourceBlob::empty(),
        std::path::Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap_err();
    assert!(err.contains("INVALID=KEY"));
    assert!(err.contains("contains '='"));
}

#[test]
fn build_target_env_accepts_non_nul_utf8_checkout_values() {
    let value = "equals=sign\ncontrol:\u{001f} Unicode: Grüße";
    let mut checkout = ResourceBlob::empty();
    checkout
        .env
        .insert("VALID_UTF8".to_string(), SecretBytes::from(value));
    let target_env = build_target_env(
        &HashMap::new(),
        &sample_exec_bundle(),
        &checkout,
        std::path::Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap();
    assert_eq!(target_env.get("VALID_UTF8"), Some(&value.to_string()));
}

#[test]
fn build_target_env_rejects_nul_passthrough_before_libsecret_helper_setup() {
    let mut bundle = sample_exec_bundle();
    bundle.resource.libsecret = true;
    let host_env = HashMap::from([("LANG".to_string(), "en_US.UTF-8\0bad".to_string())]);
    let err = build_target_env(
        &host_env,
        &bundle,
        &ResourceBlob::empty(),
        std::path::Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap_err();
    assert!(err.contains("LANG"));
    assert!(err.contains("NUL"));
    assert!(!err.contains("en_US.UTF-8"));
}
