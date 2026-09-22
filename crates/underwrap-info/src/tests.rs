use super::*;
use serde_json::{Value, json};

fn wrapper(instance: &str, name: &str, users: &[&str]) -> Value {
    json!({
        "instance": instance,
        "name": name,
        "description": "Read and write task documents",
        "command": [format!("/run/wrappers/bin/underwrap-{instance}"), name],
        "alias": {"name": name, "users": users},
        "access_paths": ["~", "/srv/tasks"],
        "projections": {"env": false, "files": [".config/tool"], "libsecret": true},
        "fixed_home": false,
        "fixed_tmpdir": false
    })
}

fn fixture() -> Value {
    json!({
        "schema_version": 1,
        "instances": ["work", "empty", "other"],
        "wrappers": [
            wrapper("work", "docs", &["alice"]),
            wrapper("other", "docs", &[]),
            wrapper("work", "calendar", &["alice"])
        ]
    })
}

fn execute(value: &Value, args: &[&str]) -> Result<String, Error> {
    let cli = Cli::try_parse_from(std::iter::once("underwrap-info").chain(args.iter().copied()))
        .expect("valid test CLI");
    let bytes = serde_json::to_vec(value).unwrap();
    let mut output = Vec::new();
    run(&cli, bytes.as_slice(), &mut output)?;
    Ok(String::from_utf8(output).unwrap())
}

#[test]
fn inventory_includes_wrappers_without_aliases_and_sorts_by_instance_and_name() {
    let output: Value = serde_json::from_str(&execute(&fixture(), &["--json"]).unwrap()).unwrap();
    assert_eq!(output["schema_version"], 1);
    assert_eq!(output["scope"], "local-configuration");
    assert_eq!(output["authorization"], "not-checked");
    assert_eq!(output["filesystem_access"], "not-checked");
    assert_eq!(output["instances"], json!(["empty", "other", "work"]));
    assert_eq!(
        output["wrappers"],
        json!([
            wrapper("other", "docs", &[]),
            wrapper("work", "calendar", &["alice"]),
            wrapper("work", "docs", &["alice"])
        ])
    );
}

#[test]
fn text_inventory_names_commands_descriptions_alias_intent_and_limits() {
    let output = execute(&fixture(), &[]).unwrap();
    assert!(output.starts_with(NOTICE));
    assert!(output.contains("other/docs — Read and write task documents"));
    assert!(output.contains("Command: /run/wrappers/bin/underwrap-other docs"));
    assert!(output.contains("Alias: docs (configured for users: (none))"));
    assert!(output.contains("Alias: calendar (configured for users: alice)"));
    assert!(!output.contains("ID-mapped host paths:"));
}

#[test]
fn wrapper_selection_requires_disambiguation_without_guessing_current_user() {
    assert_eq!(execute(&fixture(), &["docs"]), Err(Error::AmbiguousWrapper));
    let output: Value = serde_json::from_str(
        &execute(&fixture(), &["docs", "--instance", "other", "--json"]).unwrap(),
    )
    .unwrap();
    assert_eq!(output["wrappers"], json!([wrapper("other", "docs", &[])]));
    assert_eq!(output["instances"], json!(["other"]));
}

#[test]
fn instance_filter_and_unique_wrapper_selection_work_independently() {
    let output: Value =
        serde_json::from_str(&execute(&fixture(), &["--instance", "work", "--json"]).unwrap())
            .unwrap();
    assert_eq!(output["wrappers"].as_array().unwrap().len(), 2);
    let output: Value =
        serde_json::from_str(&execute(&fixture(), &["calendar", "--json"]).unwrap()).unwrap();
    assert_eq!(
        output["wrappers"],
        json!([wrapper("work", "calendar", &["alice"])])
    );
}

#[test]
fn empty_known_instance_is_not_an_unknown_instance_or_missing_wrapper() {
    let output = execute(&fixture(), &["--instance", "empty"]).unwrap();
    assert!(output.contains("No wrappers configured"));
    let output: Value =
        serde_json::from_str(&execute(&fixture(), &["--instance", "empty", "--json"]).unwrap())
            .unwrap();
    assert_eq!(output["instances"], json!(["empty"]));
    assert_eq!(output["wrappers"], json!([]));
    assert_eq!(
        execute(&fixture(), &["--instance", "missing"]),
        Err(Error::UnknownInstance)
    );
    assert_eq!(
        execute(&fixture(), &["missing"]),
        Err(Error::UnknownWrapper)
    );
    assert_eq!(
        execute(&fixture(), &["docs", "--instance", "empty"]),
        Err(Error::UnknownWrapper)
    );
}

#[test]
fn empty_catalog_is_a_successful_inventory() {
    let fixture = json!({"schema_version": 1, "instances": [], "wrappers": []});
    let output = execute(&fixture, &[]).unwrap();
    assert!(output.contains("No wrappers configured"));
    let output: Value = serde_json::from_str(&execute(&fixture, &["--json"]).unwrap()).unwrap();
    assert_eq!(output["wrappers"], json!([]));
}

#[test]
fn detail_distinguishes_host_paths_from_projection_paths_and_fixed_environment() {
    let mut fixture = fixture();
    fixture["wrappers"][2]["fixed_home"] = json!(true);
    fixture["wrappers"][2]["fixed_tmpdir"] = json!(true);
    let output = execute(&fixture, &["calendar"]).unwrap();
    assert!(output.contains("ID-mapped host paths: ~, /srv/tasks"));
    assert!(output.contains("Files projection paths (caller passwd-home relative): .config/tool"));
    assert!(output.contains("Fixed HOME override: true"));
    assert!(output.contains("Fixed TMPDIR override: true"));
    assert!(output.contains("TMPDIR does not rewrite explicit paths"));
}

#[test]
fn missing_description_has_an_explicit_text_fallback_but_json_preserves_empty_string() {
    let mut fixture = fixture();
    fixture["wrappers"][2]["description"] = json!("");
    assert!(
        execute(&fixture, &["calendar"])
            .unwrap()
            .contains("(no description)")
    );
    let output: Value =
        serde_json::from_str(&execute(&fixture, &["calendar", "--json"]).unwrap()).unwrap();
    assert_eq!(output["wrappers"][0]["description"], "");
}

#[test]
fn public_operator_text_is_escaped_for_terminals_but_roundtrips_through_json() {
    let mut fixture = fixture();
    let text = "Documents: Grüße \"quoted\"\n\t\r\u{1b}[2J\u{202e}";
    fixture["wrappers"][2]["description"] = json!(text);
    fixture["wrappers"][2]["alias"]["users"] = json!(["user\u{1b}[2J"]);
    let output = execute(&fixture, &["calendar"]).unwrap();
    assert!(output.contains("Grüße"));
    assert!(!output.contains('\u{1b}'));
    assert!(!output.contains('\u{202e}'));
    assert!(!output.contains('\r'));
    assert!(!output.contains('\t'));
    assert!(output.contains("\\n"));
    let output: Value =
        serde_json::from_str(&execute(&fixture, &["calendar", "--json"]).unwrap()).unwrap();
    assert_eq!(output["wrappers"][0]["description"], text);
}

#[test]
fn malformed_catalogs_are_rejected_without_rendering_partial_metadata() {
    for bytes in [
        b"not json".as_slice(),
        b"{}",
        b"[]",
        b"null",
        b"{\"schema_version\":",
    ] {
        let cli = Cli::try_parse_from(["underwrap-info", "--json"]).unwrap();
        let mut output = Vec::new();
        assert_eq!(run(&cli, bytes, &mut output), Err(Error::InvalidCatalog));
        assert!(output.is_empty());
    }
}

#[test]
fn selection_errors_never_emit_a_partial_json_inventory() {
    let bytes = serde_json::to_vec(&fixture()).unwrap();
    for args in [
        vec!["docs", "--json"],
        vec!["missing", "--json"],
        vec!["--instance", "missing", "--json"],
    ] {
        let cli = Cli::try_parse_from(std::iter::once("underwrap-info").chain(args)).unwrap();
        let mut output = Vec::new();
        assert!(run(&cli, bytes.as_slice(), &mut output).is_err());
        assert!(output.is_empty());
    }
}

#[test]
fn invalid_field_types_return_only_the_static_catalog_diagnostic() {
    let mut value = fixture();
    value["wrappers"][0]["fixed_tmpdir"] = json!("invalid-field-value");
    let error = execute(&value, &["--json"]).unwrap_err();
    assert_eq!(error, Error::InvalidCatalog);
    assert_eq!(
        error.message(),
        "invalid discovery catalog; ask the host operator to rebuild the package and module together"
    );
}

#[test]
fn version_and_extra_fields_fail_closed() {
    let mut value = fixture();
    value["schema_version"] = json!(2);
    assert_eq!(execute(&value, &[]), Err(Error::UnsupportedVersion));
    for field_path in [
        vec![],
        vec!["wrappers", "0"],
        vec!["wrappers", "0", "alias"],
        vec!["wrappers", "0", "projections"],
    ] {
        let mut value = fixture();
        let mut object = &mut value;
        for field in field_path {
            object = if field == "0" {
                &mut object[0]
            } else {
                &mut object[field]
            };
        }
        object["unexpected"] = json!(true);
        assert_eq!(execute(&value, &[]), Err(Error::InvalidCatalog));
    }
}

#[test]
fn duplicate_or_inconsistent_metadata_is_rejected() {
    let mut duplicate_instance = fixture();
    duplicate_instance["instances"]
        .as_array_mut()
        .unwrap()
        .push(json!("work"));
    let mut duplicate_wrapper = fixture();
    duplicate_wrapper["wrappers"]
        .as_array_mut()
        .unwrap()
        .push(wrapper("work", "docs", &[]));
    let mut unknown_instance = fixture();
    unknown_instance["wrappers"][0]["instance"] = json!("absent");
    let mut bad_alias = fixture();
    bad_alias["wrappers"][0]["alias"]["name"] = json!("different");
    let mut bad_command = fixture();
    bad_command["wrappers"][0]["command"] = json!(["/bin/false", "docs"]);
    for value in [
        duplicate_instance,
        duplicate_wrapper,
        unknown_instance,
        bad_alias,
        bad_command,
    ] {
        assert_eq!(execute(&value, &[]), Err(Error::InvalidCatalog));
    }
}

#[test]
fn selector_grammar_matches_the_module_boundaries() {
    for name in ["a", "work-team_1", &"a".repeat(21)] {
        assert!(instance_name(name));
    }
    for name in ["", "A", "1a", "x.y", "x/y", "x\n", &"a".repeat(22)] {
        assert!(!instance_name(name));
    }
    for name in ["a", "1", "A.b_c+d-e", &"a".repeat(128)] {
        assert!(wrapper_name(name));
    }
    for name in ["", "-flag", "debug", "a/b", "x y", "ä", &"a".repeat(129)] {
        assert!(!wrapper_name(name));
        let mut value = fixture();
        value["wrappers"][0]["name"] = json!(name);
        assert_eq!(execute(&value, &[]), Err(Error::InvalidCatalog));
    }
}

#[test]
fn usage_and_help_do_not_require_a_catalog_and_no_probe_or_config_override_exists() {
    for args in [vec!["--help"], vec!["--version"]] {
        let error = Cli::try_parse_from(std::iter::once("underwrap-info").chain(args)).unwrap_err();
        assert_eq!(error.exit_code(), 0);
    }
    for args in [
        vec!["--catalog", "/tmp/catalog"],
        vec!["--probe"],
        vec!["docs", "--", "run"],
    ] {
        assert!(Cli::try_parse_from(std::iter::once("underwrap-info").chain(args)).is_err());
    }
}

#[test]
fn io_errors_remain_errors() {
    struct FailedIo;
    impl Read for FailedIo {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("test read error"))
        }
    }
    impl Write for FailedIo {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("test write error"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert!(matches!(read_catalog(FailedIo), Err(Error::InvalidCatalog)));
    for args in [vec![], vec!["--json"]] {
        let cli = Cli::try_parse_from(std::iter::once("underwrap-info").chain(args)).unwrap();
        let bytes = serde_json::to_vec(&fixture()).unwrap();
        assert_eq!(
            run(&cli, bytes.as_slice(), FailedIo),
            Err(Error::OutputFailed)
        );
    }
}
