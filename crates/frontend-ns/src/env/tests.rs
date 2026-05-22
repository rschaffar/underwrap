use super::*;
use underwrap_protocol::config::AccessMode;
use underwrap_protocol::exec::ExecResourceBinding;

fn sample_binding() -> ExecResourceBinding {
    ExecResourceBinding {
        resource_id: "res_x_y".to_string(),
        resource_name: "x/y".to_string(),
        access: AccessMode::ReadOnly,
        channel_fd: 20,
        libsecret: false,
        files_target_paths: Vec::new(),
    }
}

fn sample_exec_bundle() -> ExecBundle {
    ExecBundle {
        env: HashMap::from([("GH_TOKEN".to_string(), "ghp_test".into())]),
        env_passthrough: vec!["CUSTOM_PASSTHROUGH".to_string()],
        env_set: HashMap::from([("PATH".to_string(), "/trusted/bin".to_string())]),
        resource: sample_binding(),
    }
}

#[test]
fn build_target_env_applies_whitelist_passthrough_env_set_and_secrets() {
    let host_env = HashMap::from([
        ("HOME".to_string(), "/home/alice".to_string()),
        ("LANG".to_string(), "en_US.UTF-8".to_string()),
        ("PATH".to_string(), "/untrusted/bin".to_string()),
        ("LD_PRELOAD".to_string(), "/tmp/evil.so".to_string()),
        ("CUSTOM_PASSTHROUGH".to_string(), "allowed".to_string()),
    ]);

    let target_env = build_target_env(&host_env, &sample_exec_bundle()).unwrap();
    assert_eq!(target_env.get("HOME"), Some(&"/home/alice".to_string()));
    assert_eq!(target_env.get("LANG"), Some(&"en_US.UTF-8".to_string()));
    assert_eq!(
        target_env.get("CUSTOM_PASSTHROUGH"),
        Some(&"allowed".to_string())
    );
    assert_eq!(target_env.get("PATH"), Some(&"/trusted/bin".to_string()));
    assert_eq!(target_env.get("GH_TOKEN"), Some(&"ghp_test".to_string()));
    assert!(!target_env.contains_key("LD_PRELOAD"));
}

#[test]
fn build_target_env_rejects_non_utf8_secret_values() {
    let host_env = HashMap::new();
    let bundle = ExecBundle {
        env: HashMap::from([("BAD".to_string(), vec![0xff, 0xfe].into())]),
        env_passthrough: Vec::new(),
        env_set: HashMap::new(),
        resource: sample_binding(),
    };

    let err = build_target_env(&host_env, &bundle).unwrap_err();
    assert!(err.contains("not valid UTF-8"));
    assert!(err.contains("BAD"));
}
