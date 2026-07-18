use super::*;
use std::collections::HashMap;
use underwrap_protocol::exec::ExecResourceBinding;

fn bundle(access_paths: Vec<String>) -> ExecBundle {
    ExecBundle {
        env: HashMap::new(),
        env_passthrough: Vec::new(),
        env_set: HashMap::new(),
        target_runtime_dir_base: "/run/underwrap-runtime".to_string(),
        resource: ExecResourceBinding {
            resource_id: "res_x_y".to_string(),
            resource_name: "x/y".to_string(),
            access: underwrap_protocol::config::AccessMode::ReadOnly,
            libsecret: false,
            files_target_paths: Vec::new(),
        },
        access_paths,
    }
}

#[test]
fn target_runtime_dir_is_base_plus_target_user_name() {
    // ns derives the executing uid's own dir as <base>/<user-name>. uid 0 maps
    // to "root" on every Unix, so this is a portable check of the scheme.
    assert_eq!(
        target_runtime_dir("/run/underwrap-runtime", 0).unwrap(),
        "/run/underwrap-runtime/root",
    );
}

#[test]
fn target_runtime_dir_rejects_relative_base() {
    assert!(target_runtime_dir("run/underwrap-runtime", 0).is_err());
}

#[test]
fn build_target_env_uses_the_resolved_runtime_dir_verbatim() {
    // build_target_env does not derive the runtime dir; it exports exactly the
    // value run() resolved (the target's own provisioned dir).
    let b = bundle(Vec::new());
    let env = crate::env::build_target_env(
        &HashMap::new(),
        &b,
        Path::new("/home/alice"),
        "/run/underwrap-runtime/underwrap-target-abc",
    )
    .unwrap();
    assert_eq!(
        env.get("XDG_RUNTIME_DIR").map(String::as_str),
        Some("/run/underwrap-runtime/underwrap-target-abc"),
    );
}
