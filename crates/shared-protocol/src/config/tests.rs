use super::*;

#[test]
fn minimal_config() {
    let yaml = r#"
backend:
  url: https://backend.example.internal:8443
  ca_file: /etc/underwrap/ca.pem
  client_cert_file: /etc/underwrap/client.pem
  client_key_file: /etc/underwrap/client.key
"#;
    let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
    assert!(config.wrappers.is_empty());
    assert_eq!(config.backend.url, "https://backend.example.internal:8443");
    assert_eq!(
        config.backend.ca_file,
        Some(PathBuf::from("/etc/underwrap/ca.pem"))
    );
    assert_eq!(
        config.backend.client_cert_file,
        Some(PathBuf::from("/etc/underwrap/client.pem"))
    );
    assert_eq!(
        config.backend.client_key_file,
        Some(PathBuf::from("/etc/underwrap/client.key"))
    );
}

#[test]
fn full_wrapper_config() {
    let yaml = r#"
backend:
  url: https://backend.example.internal:8443
  ca_file: /etc/underwrap/ca.pem
  client_cert_file: /etc/underwrap/client.pem
  client_key_file: /etc/underwrap/client.key
wrappers:
  gh-authorized:
    target: /usr/bin/gh
    socket: /run/underwrap/gh-authorized.sock
    group: underwrap-gh-authorized
    resource: github/token
    env: true
  gws-auth:
    target: /usr/bin/gws
    socket: /run/underwrap/gws-auth.sock
    group: underwrap-gws-auth
    resource: gws/state
    files:
      paths:
        - .config/gws
        - .local/share/gws
"#;
    let config: DaemonConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.wrappers.len(), 2);
    let gh = &config.wrappers["gh-authorized"];
    assert_eq!(gh.target, PathBuf::from("/usr/bin/gh"));
    assert_eq!(gh.resource, "github/token");
    assert!(gh.env);
    assert!(gh.files.is_none());
    assert!(!gh.libsecret);
    let gws = &config.wrappers["gws-auth"];
    assert_eq!(gws.resource, "gws/state");
    assert!(!gws.env);
    assert_eq!(
        gws.files.as_ref().unwrap().paths,
        vec![".config/gws".to_string(), ".local/share/gws".to_string()],
    );
}

#[test]
fn wrapper_rejects_unknown_fields() {
    let yaml = r#"
backend:
  url: https://x
  ca_file: /x
  client_cert_file: /y
  client_key_file: /z
wrappers:
  bad:
    target: /usr/bin/true
    socket: /run/x.sock
    group: g
    resource: github/token
    variable: GH_TOKEN
"#;
    let err = serde_yaml::from_str::<DaemonConfig>(yaml).unwrap_err();
    assert!(err.to_string().contains("unknown field `variable`"));
}

#[test]
fn wrapper_rejects_legacy_resources_map() {
    // The old multi-resource shape (`resources: { ... }`) is rejected
    // by `deny_unknown_fields` so stale configs fail loudly instead
    // of silently dropping projection slots.
    let yaml = r#"
backend:
  url: https://x
  ca_file: /x
  client_cert_file: /y
  client_key_file: /z
wrappers:
  bad:
    target: /usr/bin/true
    socket: /run/x.sock
    group: g
    resources:
      github/token:
        env: true
"#;
    let err = serde_yaml::from_str::<DaemonConfig>(yaml).unwrap_err();
    assert!(err.to_string().contains("unknown field `resources`"));
}

#[test]
fn wrapper_requires_resource() {
    let yaml = r#"
backend:
  url: https://x
  ca_file: /x
  client_cert_file: /y
  client_key_file: /z
wrappers:
  bad:
    target: /usr/bin/true
    socket: /run/x.sock
    group: g
    env: true
"#;
    let err = serde_yaml::from_str::<DaemonConfig>(yaml).unwrap_err();
    assert!(err.to_string().contains("missing field `resource`"));
}

#[test]
fn files_projection_requires_paths() {
    let yaml = r#"
backend:
  url: https://x
  ca_file: /x
  client_cert_file: /y
  client_key_file: /z
wrappers:
  bad:
    target: /usr/bin/true
    socket: /run/x.sock
    group: g
    resource: gws/state
    files: {}
"#;
    let err = serde_yaml::from_str::<DaemonConfig>(yaml).unwrap_err();
    assert!(err.to_string().contains("missing field `paths`"));
}

#[test]
fn shipped_examples_config_yaml_parses() {
    // Locks the relationship between the annotated example we ship
    // and the actual config schema. If a future change drops or
    // renames a field used in `examples/config.yaml`, this test
    // surfaces the drift instead of letting the example rot.
    //
    // Skips silently when `examples/` isn't in the build sandbox
    // (e.g. nix builds that copy only `crates/` into the source
    // tree). The drift check still fires under a normal
    // workspace-root `cargo test`, which is where humans run it.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let repo_root = std::path::Path::new(manifest)
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let example = repo_root.join("examples").join("config.yaml");
    let Ok(yaml) = std::fs::read_to_string(&example) else {
        eprintln!(
            "skipping: {} not reachable from this build sandbox",
            example.display()
        );
        return;
    };
    let _: DaemonConfig = serde_yaml::from_str(&yaml).unwrap_or_else(|e| {
        panic!("examples/config.yaml does not parse against DaemonConfig: {e}")
    });
}
