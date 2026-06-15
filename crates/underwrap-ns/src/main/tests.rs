use super::*;
use std::collections::HashMap;
use underwrap_protocol::exec::ExecResourceBinding;

fn bundle(access_paths: Vec<String>, access_runtime_dir: bool) -> ExecBundle {
    ExecBundle {
        env: HashMap::new(),
        env_passthrough: Vec::new(),
        env_set: HashMap::new(),
        xdg_runtime_dir_base: "/run/user".to_string(),
        resource: ExecResourceBinding {
            resource_id: "res_x_y".to_string(),
            resource_name: "x/y".to_string(),
            access: underwrap_protocol::config::AccessMode::ReadOnly,
            channel_fd: 20,
            libsecret: false,
            files_target_paths: Vec::new(),
        },
        access_paths,
        access_runtime_dir,
    }
}

#[test]
fn build_idmap_paths_omits_runtime_dir_when_flag_unset() {
    // Even with the dir present, an opted-out wrapper gets only its access_paths.
    let b = bundle(vec!["~/.config/gh".to_string()], false);
    assert_eq!(
        build_idmap_paths(&b, 1700, true),
        vec!["~/.config/gh".to_string()]
    );
}

#[test]
fn build_idmap_paths_appends_caller_runtime_dir_when_flag_set_and_present() {
    let b = bundle(vec!["~".to_string()], true);
    assert_eq!(
        build_idmap_paths(&b, 1700, true),
        vec!["~".to_string(), "/run/user/1700".to_string()],
    );
}

#[test]
fn build_idmap_paths_skips_runtime_dir_when_absent() {
    // Default-on but the caller has no runtime dir (e.g. no logind session):
    // skip it rather than hard-fail, so the wrapper still runs.
    let b = bundle(vec!["~".to_string()], true);
    assert_eq!(build_idmap_paths(&b, 1700, false), vec!["~".to_string()]);
}

#[test]
fn build_idmap_paths_resolves_runtime_dir_from_caller_uid_not_wire() {
    // The appended runtime dir uses the uid passed in (ns's own getuid()), so it
    // tracks the actual caller; nothing on the wire selects the uid.
    let b = bundle(Vec::new(), true);
    assert_eq!(
        build_idmap_paths(&b, 31337, true),
        vec!["/run/user/31337".to_string()]
    );
}

#[test]
fn build_idmap_paths_runtime_dir_matches_xdg_runtime_dir_env() {
    // The idmapped path must be byte-identical to the XDG_RUNTIME_DIR env.rs
    // sets, or the target would get a runtime dir it cannot use.
    let b = bundle(Vec::new(), true);
    let env = crate::env::build_target_env(
        &HashMap::new(),
        &b,
        std::path::Path::new("/home/alice"),
        1700,
    )
    .unwrap();
    assert_eq!(
        build_idmap_paths(&b, 1700, true).last().map(String::as_str),
        env.get("XDG_RUNTIME_DIR").map(String::as_str),
    );
}

#[test]
fn build_idmap_paths_empty_when_nothing_requested() {
    let b = bundle(Vec::new(), false);
    assert!(build_idmap_paths(&b, 1700, true).is_empty());
}
