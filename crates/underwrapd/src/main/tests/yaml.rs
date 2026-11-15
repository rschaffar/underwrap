use super::*;

const HEADER: &str = "backend: {url: 'https://backend.example'}\ninstance: local\nsocket: /run/underwrap/local.sock\ngroup: underwrap-local\ndebug_tool_target: /bin/true\ntarget_runtime_dir_base: /run/underwrap-runtime\n";
const WRAPPER: &str = "{target: /bin/true, resource: 'null', env: true}";

#[test]
fn yaml_json_defaults_and_meaningful_strings_round_trip() {
    let yaml = format!(
        "{HEADER}wrappers:\n  probe:\n    target: /bin/true\n    resource: 'null'\n    env: true\n    env_set: {{TEXT: '雪 ☃', NULL_TEXT: '~', BOOL_TEXT: 'true', EMPTY: ''}}\n"
    );
    let config = parse_daemon_config(&yaml).unwrap();
    config.validate().unwrap();
    let wrapper = &config.wrappers["probe"];
    assert!(wrapper.files.is_none());
    assert!(!wrapper.libsecret);
    assert!(wrapper.env_passthrough.is_empty());
    assert_eq!(wrapper.resource, "null");
    assert_eq!(wrapper.env_set["NULL_TEXT"], "~");
    assert_eq!(wrapper.env_set["TEXT"], "雪 ☃");
    assert_eq!(wrapper.env_set["BOOL_TEXT"], "true");
    assert_eq!(wrapper.env_set["EMPTY"], "");
    assert!(config.backend.ca_file.is_none());
    assert!(config.backend.client_identity_file.is_none());
    let expected = serde_json::to_value(&config).unwrap();
    for text in [
        serde_saphyr::to_string(&config).unwrap(),
        serde_json::to_string(&config).unwrap(),
    ] {
        let restored = parse_daemon_config(&text).unwrap();
        restored.validate().unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), expected);
    }
    let defaults = parse_daemon_config(HEADER).unwrap();
    assert!(defaults.wrappers.is_empty());
    let optional_nulls = parse_daemon_config(&HEADER.replace(
        "{url: 'https://backend.example'}",
        "{url: 'https://backend.example', ca_file: null, client_identity_file: null}",
    ))
    .unwrap();
    assert!(optional_nulls.backend.ca_file.is_none());
    assert!(optional_nulls.backend.client_identity_file.is_none());
}

#[test]
fn shipped_example_uses_production_parser_and_round_trips() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/daemon-config.yaml");
    let config = parse_daemon_config(&std::fs::read_to_string(path).unwrap()).unwrap();
    config.validate().unwrap();
    let restored = parse_daemon_config(&serde_saphyr::to_string(&config).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(
        serde_json::to_value(config).unwrap(),
        serde_json::to_value(restored).unwrap()
    );
}

#[test]
fn rejects_duplicate_fields_and_map_keys_including_ignored_subtrees() {
    for extra in [
        "instance: other\n".to_owned(),
        format!("wrappers: {{probe: {WRAPPER}, probe: {WRAPPER}}}\n"),
        "wrappers: {probe: {target: /bin/true, resource: r, env: true, env_set: {TEXT: a, TEXT: b}}}\n".to_owned(),
        "ignored: {nested: {same: a, same: b}}\n".to_owned(),
    ] {
        let error = parse_daemon_config(&format!("{HEADER}{extra}")).unwrap_err();
        assert!(format!("{error:#}").to_lowercase().contains("duplicate"));
    }
}

#[test]
fn rejects_schema_errors_and_multiple_documents() {
    for text in [
        HEADER.replace("instance: local\n", ""),
        HEADER.replace("{url: 'https://backend.example'}", "{url: []}"),
        HEADER.replace(
            "{url: 'https://backend.example'}",
            "{url: 'https://backend.example', typo: x}",
        ),
        format!("{HEADER}wrappers: []\n"),
        format!("{HEADER}wrappers: {{probe: {{target: /bin/true, env: true}}}}\n"),
        format!("{HEADER}wrappers: {{probe: {{target: /bin/true, resource: r, typo: true}}}}\n"),
        format!("{HEADER}wrappers: {{probe: {{target: /bin/true, resource: r, files: {{}}}}}}\n"),
        format!("{HEADER}---\n{HEADER}"),
        format!("{HEADER}---\n"),
        format!("{HEADER}...\n@"),
    ] {
        assert!(parse_daemon_config(&text).is_err(), "accepted {text:?}");
    }
}

#[test]
fn aliases_work_but_merges_do_not_expand_and_cycles_reject() {
    let control = format!("{HEADER}wrappers: {{one: &w {WRAPPER}, two: *w}}\n");
    let config = parse_daemon_config(&control).unwrap();
    config.validate().unwrap();
    assert_eq!(config.wrappers.len(), 2);
    let merge = format!("{HEADER}template: &w {WRAPPER}\nwrappers: {{probe: {{<<: *w}}}}\n");
    for text in [merge.clone(), merge.replace("<<:", "!!merge <<:")] {
        let error = parse_daemon_config(&text).unwrap_err();
        assert!(format!("{error:#}").contains("unknown field `<<`"));
    }
    assert!(parse_daemon_config(&format!("{HEADER}ignored: &cycle [*cycle]\n")).is_err());
}

#[test]
fn native_alias_budget_remains_enabled_for_ignored_fields() {
    let input = |count| {
        format!(
            "{HEADER}ignored: {{anchor: &a ordinary, aliases: [{}]}}",
            vec!["*a"; count].join(", ")
        )
    };
    parse_daemon_config(&input(10)).unwrap();
    let error = parse_daemon_config(&input(101)).unwrap_err();
    let diagnostic = format!("{error:#}").to_lowercase();
    assert!(diagnostic.contains("alias"));
}

#[test]
fn native_depth_budget_also_applies_to_ignored_fields() {
    let nested = format!("{}0{}", "[".repeat(150), "]".repeat(150));
    let error = parse_daemon_config(&format!("{HEADER}ignored: {nested}\n")).unwrap_err();
    let diagnostic = format!("{error:#}").to_lowercase();
    assert!(diagnostic.contains("depth") || diagnostic.contains("budget"));
}
