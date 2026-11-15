use super::*;

/// Instance-level header shared by the fixtures below. One socket, one
/// group, and one debug tool per instance.
const INSTANCE_HEADER: &str = r#"
backend:
  url: https://backend.example.internal:8443
  ca_file: /etc/underwrap/ca.pem
  client_identity_file: /var/lib/underwrap-local/client-local.identity.pem
instance: local
socket: /run/underwrap/local.sock
group: underwrap-local
debug_tool_target: /run/current-system/sw/bin/underwrap-debug-tool
target_runtime_dir_base: /run/underwrap-runtime
"#;

fn with_header(body: &str) -> String {
    format!("{INSTANCE_HEADER}{body}")
}

#[test]
fn minimal_config() {
    let config: DaemonConfig = serde_saphyr::from_str(INSTANCE_HEADER).unwrap();
    assert!(config.wrappers.is_empty());
    assert_eq!(config.instance, "local");
    assert_eq!(config.socket, PathBuf::from("/run/underwrap/local.sock"));
    assert_eq!(config.group, "underwrap-local");
    assert_eq!(config.target_runtime_dir_base, "/run/underwrap-runtime");
    config.validate().unwrap();
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
    let config: DaemonConfig = serde_saphyr::from_str(&yaml).unwrap();
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
    let err = serde_saphyr::from_str::<DaemonConfig>(&yaml).unwrap_err();
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
    let err = serde_saphyr::from_str::<DaemonConfig>(&yaml).unwrap_err();
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
    let err = serde_saphyr::from_str::<DaemonConfig>(&yaml).unwrap_err();
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
    let err = serde_saphyr::from_str::<DaemonConfig>(&yaml).unwrap_err();
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
    let err = serde_saphyr::from_str::<DaemonConfig>(&yaml).unwrap_err();
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
    let config: DaemonConfig = serde_saphyr::from_str(&yaml).unwrap();
    let err = config.validate().unwrap_err();
    assert!(err.contains("reserved"), "unexpected error: {err}");
}

#[test]
fn validate_reports_first_wrapper_error_in_lexical_name_order() {
    let mut config = valid_config();
    let valid_wrapper = config.wrappers.remove("probe").unwrap();
    let mut invalid_wrapper = valid_wrapper.clone();
    invalid_wrapper.target = "relative/target".into();
    config
        .wrappers
        .insert("z-invalid".to_string(), invalid_wrapper);
    config
        .wrappers
        .insert(RESERVED_WRAPPER_NAME_DEBUG.to_string(), valid_wrapper);
    assert_invalid(
        config,
        "wrapper name 'debug' is reserved (it is the debug subcommand token)",
    );

    let mut config = valid_config();
    let valid_wrapper = config.wrappers.remove("probe").unwrap();
    let mut invalid_wrapper = valid_wrapper.clone();
    invalid_wrapper.target = "relative/target".into();
    config
        .wrappers
        .insert(RESERVED_WRAPPER_NAME_DEBUG.to_string(), valid_wrapper);
    config
        .wrappers
        .insert("a-invalid".to_string(), invalid_wrapper);
    assert_invalid(
        config,
        "wrapper 'a-invalid' target must be a canonical absolute path",
    );
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
    let config: DaemonConfig = serde_saphyr::from_str(&yaml).unwrap();
    let err = config.validate().unwrap_err();
    assert!(
        err.contains("no projection slot"),
        "unexpected error: {err}"
    );
}

fn valid_config() -> DaemonConfig {
    let yaml = with_header(
        r#"
wrappers:
  probe:
    target: /usr/bin/true
    resource: probe/resource
    env: true
    files:
      paths:
        - .config/probe
        - .local/share/probe
    env_set:
      HOME: /home/caller
      XDG_CONFIG_HOME: /home/caller/.config
      XDG_DATA_HOME: /home/caller/.local/share
      XDG_CACHE_HOME: /home/caller/.cache
      XDG_STATE_HOME: /home/caller/.local/state
      PATH: relative path remains unrestricted
    access_paths:
      - /home/caller/.config
      - "~/.config"
"#,
    );
    serde_saphyr::from_str(&yaml).unwrap()
}

fn rename_probe(config: &mut DaemonConfig, name: String) {
    let wrapper = config.wrappers.remove("probe").unwrap();
    config.wrappers.insert(name, wrapper);
}

fn assert_invalid(config: DaemonConfig, expected: &str) {
    let error = config.validate().unwrap_err();
    assert_eq!(error, expected);
}

#[test]
fn validate_accepts_exact_name_boundaries() {
    for instance in ["a".to_string(), format!("a{}", "z".repeat(20))] {
        let mut config = valid_config();
        config.instance = instance;
        config.validate().unwrap();
    }

    for wrapper in [
        "A".to_string(),
        "aZ09._+-".to_string(),
        format!("A{}", "z".repeat(127)),
    ] {
        let mut config = valid_config();
        rename_probe(&mut config, wrapper);
        config.validate().unwrap();
    }
}

#[test]
fn validate_rejects_invalid_instance_name_classes() {
    for instance in [
        "".to_string(),
        "A".to_string(),
        format!("a{}", "z".repeat(21)),
        "a.b".to_string(),
        "a b".to_string(),
        "a\tb".to_string(),
        "a\nb".to_string(),
        "é".to_string(),
    ] {
        let mut config = valid_config();
        config.instance = instance;
        assert_invalid(config, "instance name must match [a-z][a-z0-9_-]{0,20}");
    }
}

#[test]
fn validate_rejects_invalid_wrapper_name_classes() {
    for wrapper in [
        "".to_string(),
        format!("A{}", "z".repeat(128)),
        "_probe".to_string(),
        "probe/name".to_string(),
        "probe name".to_string(),
        "probe\tname".to_string(),
        "probe\nname".to_string(),
        "é".to_string(),
    ] {
        let mut config = valid_config();
        rename_probe(&mut config, wrapper);
        assert_invalid(
            config,
            "wrapper name must match [A-Za-z0-9][A-Za-z0-9._+-]{0,127}",
        );
    }
}

#[test]
fn canonical_path_predicates_cover_component_and_character_classes() {
    for path in ["/", "/a", "/a.b_c-+/child", "/café/設定"] {
        assert!(is_canonical_absolute_path(std::path::Path::new(path)));
    }
    for path in ["a", ".config/probe", "a.b_c-+/child", "café/設定"] {
        assert!(is_canonical_relative_path(path));
    }
    for path in [
        "",
        "relative",
        "//a",
        "/a/",
        "/a//b",
        "/a/./b",
        "/a/../b",
        "/a b",
        "/a\tb",
        "/a\nb",
        "/a\u{a0}b",
        "/a\u{85}b",
    ] {
        assert!(!is_canonical_absolute_path(std::path::Path::new(path)));
    }
    for path in [
        "", "/a", "a/", "a//b", "a/./b", "a/../b", "a b", "a\tb", "a\nb", "a\u{a0}b", "a\u{85}b",
    ] {
        assert!(!is_canonical_relative_path(path));
    }
}

#[test]
fn validate_accepts_exact_tilde_access_path() {
    let mut config = valid_config();
    config.wrappers.get_mut("probe").unwrap().access_paths = vec!["~".to_string()];
    config.validate().unwrap();
}

#[test]
fn validate_checks_the_complete_absolute_runtime_path_inventory() {
    type ConfigMutation = fn(&mut DaemonConfig);

    let invalid = "/valid/../invalid";
    let cases: [(&str, ConfigMutation); 6] = [
        ("socket", |config| {
            config.socket = "/valid/../invalid".into();
        }),
        ("debug_tool_target", |config| {
            config.debug_tool_target = "/valid/../invalid".into();
        }),
        ("target_runtime_dir_base", |config| {
            config.target_runtime_dir_base = "/valid/../invalid".to_string();
        }),
        ("backend.ca_file", |config| {
            config.backend.ca_file = Some("/valid/../invalid".into());
        }),
        ("backend.client_identity_file", |config| {
            config.backend.client_identity_file = Some("/valid/../invalid".into());
        }),
        ("wrapper 'probe' target", |config| {
            config.wrappers.get_mut("probe").unwrap().target = "/valid/../invalid".into();
        }),
    ];

    for (field, mutate) in cases {
        let mut config = valid_config();
        mutate(&mut config);
        assert_invalid(
            config,
            &format!("{field} must be a canonical absolute path"),
        );
    }

    for name in [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
    ] {
        let mut config = valid_config();
        config
            .wrappers
            .get_mut("probe")
            .unwrap()
            .env_set
            .insert(name.to_string(), invalid.to_string());
        assert_invalid(
            config,
            &format!("wrapper 'probe' env_set {name} must be a canonical absolute path"),
        );
    }
}

#[test]
fn validate_rejects_invalid_files_path_classes() {
    for path in [
        "",
        "/absolute",
        "a/",
        "a//b",
        "a/./b",
        "a/../b",
        "a b",
        "a\tb",
        "a\nb",
    ] {
        let mut config = valid_config();
        config.wrappers.get_mut("probe").unwrap().files = Some(FilesUse {
            paths: vec![path.to_string()],
        });
        assert_invalid(
            config,
            "wrapper 'probe' files.paths entries must be canonical relative paths",
        );
    }

    let mut config = valid_config();
    config.wrappers.get_mut("probe").unwrap().files = Some(FilesUse { paths: vec![] });
    assert_invalid(
        config,
        "wrapper 'probe' files.paths must contain at least one path",
    );

    for paths in [
        vec![".config/probe", ".config/probe"],
        vec![".config", ".config/probe"],
    ] {
        let mut config = valid_config();
        config.wrappers.get_mut("probe").unwrap().files = Some(FilesUse {
            paths: paths.into_iter().map(str::to_string).collect(),
        });
        assert_invalid(
            config,
            "wrapper 'probe' files.paths must be unique and non-overlapping",
        );
    }
}

#[test]
fn validate_rejects_invalid_access_path_classes_and_same_domain_overlap() {
    for path in [
        "relative", "~/", "~/a/", "~/a//b", "~/a/./b", "~/a/../b", "~/a b", "/a/", "/a//b",
        "/a/./b", "/a/../b", "/a\tb",
    ] {
        let mut config = valid_config();
        config.wrappers.get_mut("probe").unwrap().access_paths = vec![path.to_string()];
        assert_invalid(
            config,
            "wrapper 'probe' access_paths entries must be canonical absolute paths, '~', or canonical '~/...' paths",
        );
    }

    for paths in [
        vec!["/a", "/a"],
        vec!["/", "/a"],
        vec!["/a", "/a/b"],
        vec!["~", "~"],
        vec!["~", "~/a"],
        vec!["~/a", "~/a/b"],
    ] {
        let mut config = valid_config();
        config.wrappers.get_mut("probe").unwrap().access_paths =
            paths.into_iter().map(str::to_string).collect();
        assert_invalid(
            config,
            "wrapper 'probe' access_paths must be unique and non-overlapping within the absolute and tilde domains",
        );
    }
}

#[test]
fn validate_preserves_unrestricted_fields_and_mixed_access_domains() {
    let mut config = valid_config();
    config.backend.url = "backend URL validation remains separate".into();
    let wrapper = config.wrappers.get_mut("probe").unwrap();
    wrapper.resource = "resource name\tgrammar remains unchanged".into();
    wrapper.access_paths = vec!["/home/caller/.config".into(), "~/.config".into()];
    wrapper
        .env_set
        .insert("PATH".into(), "relative path with whitespace".into());
    wrapper
        .env_set
        .insert("ARBITRARY".into(), "not/a/canonical/path/../value".into());
    config.validate().unwrap();
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
    let _: DaemonConfig = serde_saphyr::from_str(&yaml).unwrap_or_else(|e| {
        panic!("examples/daemon-config.yaml does not parse against DaemonConfig: {e}")
    });
}
