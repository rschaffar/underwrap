use super::*;

/// Instance-level header shared by the fixtures below. One socket, one
/// group, one ns binary, one debug tool per instance.
const INSTANCE_HEADER: &str = r#"
backend:
  url: https://backend.example.internal:8443
  ca_file: /etc/underwrap/ca.pem
  client_identity_file: /var/lib/underwrap-local/client-local.identity.pem
instance: local
socket: /run/underwrap/local.sock
group: underwrap-local
ns_binary: /run/wrappers/bin/underwrap-ns
debug_tool_target: /run/current-system/sw/bin/underwrap-debug-tool
target_runtime_dir_base: /run/user
"#;

fn with_header(body: &str) -> String {
    format!("{INSTANCE_HEADER}{body}")
}

#[test]
fn minimal_config() {
    let config: DaemonConfig = serde_yaml::from_str(INSTANCE_HEADER).unwrap();
    assert!(config.wrappers.is_empty());
    assert_eq!(config.instance, "local");
    assert_eq!(config.socket, PathBuf::from("/run/underwrap/local.sock"));
    assert_eq!(config.group, "underwrap-local");
    assert_eq!(
        config.ns_binary,
        PathBuf::from("/run/wrappers/bin/underwrap-ns")
    );
    assert_eq!(config.target_runtime_dir_base, "/run/user");
    assert_eq!(config.backend.url, "https://backend.example.internal:8443");
}

#[test]
fn full_wrapper_config() {
    let yaml = with_header(
        r#"
wrappers:
  gh-authorized:
    target: /usr/bin/gh
    resource: github/token
    env: true
    env_passthrough:
      - TERM
    env_set:
      PATH: /run/current-system/sw/bin
  gws-auth:
    target: /usr/bin/gws
    resource: gws/state
    files:
      paths:
        - .config/gws
        - .local/share/gws
"#,
    );
    let config: DaemonConfig = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(config.wrappers.len(), 2);
    let gh = &config.wrappers["gh-authorized"];
    assert_eq!(gh.target, PathBuf::from("/usr/bin/gh"));
    assert_eq!(gh.resource, "github/token");
    assert!(gh.env);
    assert!(gh.files.is_none());
    assert!(!gh.libsecret);
    assert_eq!(gh.env_passthrough, vec!["TERM".to_string()]);
    assert_eq!(
        gh.env_set.get("PATH").map(String::as_str),
        Some("/run/current-system/sw/bin")
    );
    let gws = &config.wrappers["gws-auth"];
    assert_eq!(gws.resource, "gws/state");
    assert!(!gws.env);
    assert_eq!(
        gws.files.as_ref().unwrap().paths,
        vec![".config/gws".to_string(), ".local/share/gws".to_string()],
    );
    config.validate().unwrap();
}

#[test]
fn wrapper_rejects_unknown_fields() {
    let yaml = with_header(
        r#"
wrappers:
  bad:
    target: /usr/bin/true
    resource: github/token
    variable: GH_TOKEN
"#,
    );
    let err = serde_yaml::from_str::<DaemonConfig>(&yaml).unwrap_err();
    assert!(err.to_string().contains("unknown field `variable`"));
}

#[test]
fn wrapper_rejects_legacy_socket_and_group_fields() {
    // socket/group moved to instance level; a per-wrapper socket/group is
    // now an unknown field so stale configs fail loudly.
    let yaml = with_header(
        r#"
wrappers:
  bad:
    target: /usr/bin/true
    resource: github/token
    socket: /run/x.sock
"#,
    );
    let err = serde_yaml::from_str::<DaemonConfig>(&yaml).unwrap_err();
    assert!(err.to_string().contains("unknown field `socket`"));
}

#[test]
fn wrapper_rejects_legacy_resources_map() {
    let yaml = with_header(
        r#"
wrappers:
  bad:
    target: /usr/bin/true
    resources:
      github/token:
        env: true
"#,
    );
    let err = serde_yaml::from_str::<DaemonConfig>(&yaml).unwrap_err();
    assert!(err.to_string().contains("unknown field `resources`"));
}

#[test]
fn wrapper_requires_resource() {
    let yaml = with_header(
        r#"
wrappers:
  bad:
    target: /usr/bin/true
    env: true
"#,
    );
    let err = serde_yaml::from_str::<DaemonConfig>(&yaml).unwrap_err();
    assert!(err.to_string().contains("missing field `resource`"));
}

#[test]
fn files_projection_requires_paths() {
    let yaml = with_header(
        r#"
wrappers:
  bad:
    target: /usr/bin/true
    resource: gws/state
    files: {}
"#,
    );
    let err = serde_yaml::from_str::<DaemonConfig>(&yaml).unwrap_err();
    assert!(err.to_string().contains("missing field `paths`"));
}

#[test]
fn validate_rejects_reserved_debug_wrapper_name() {
    let yaml = with_header(
        r#"
wrappers:
  debug:
    target: /usr/bin/true
    resource: github/token
    env: true
"#,
    );
    let config: DaemonConfig = serde_yaml::from_str(&yaml).unwrap();
    let err = config.validate().unwrap_err();
    assert!(err.contains("reserved"), "unexpected error: {err}");
}

#[test]
fn validate_rejects_wrapper_with_no_projection_slot() {
    let yaml = with_header(
        r#"
wrappers:
  noslot:
    target: /usr/bin/true
    resource: github/token
"#,
    );
    let config: DaemonConfig = serde_yaml::from_str(&yaml).unwrap();
    let err = config.validate().unwrap_err();
    assert!(
        err.contains("no projection slot"),
        "unexpected error: {err}"
    );
}

#[test]
fn shipped_daemon_config_example_parses() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let repo_root = std::path::Path::new(manifest)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let example = repo_root.join("examples").join("daemon-config.yaml");
    let yaml = std::fs::read_to_string(&example).unwrap_or_else(|error| {
        panic!("cannot read shipped example {}: {error}", example.display())
    });
    let _: DaemonConfig = serde_yaml::from_str(&yaml).unwrap_or_else(|e| {
        panic!("examples/daemon-config.yaml does not parse against DaemonConfig: {e}")
    });
}
