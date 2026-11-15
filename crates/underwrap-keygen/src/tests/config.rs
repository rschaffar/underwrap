use super::*;

fn resolver() -> FixedResolver {
    FixedResolver {
        uid: Ok(Some(123)),
        gid: Ok(Some(456)),
        names: Vec::new(),
    }
}

fn config_cli(text: &str) -> (tempfile::TempDir, Cli) {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("work.yaml");
    std::fs::write(&path, text).unwrap();
    (
        tmp,
        Cli {
            client_identity_out: None,
            instance_config: Some(path),
        },
    )
}

#[test]
fn backend_only_yaml_and_json_use_filename_not_content_for_owner() {
    for text in [
        "backend: {url: 'https://backend.example', client_identity_file: '/var/lib/秘密/身份.pem', ca_file: null}\ninstance: misleading\nwrappers: [not, a, daemon, schema]\nunrelated: {nested: [true, null]}\n",
        r#"{"backend":{"url":"https://backend.example","client_identity_file":"/var/lib/秘密/身份.pem","ca_file":null},"instance":"misleading","wrappers":false}"#,
    ] {
        let (_tmp, cli) = config_cli(text);
        let mut resolver = resolver();
        let targets = resolve_targets_with(&cli, &mut resolver).unwrap();
        validate_output_path(&targets.client_identity_out).unwrap();
        assert_eq!(
            targets.client_identity_out,
            PathBuf::from("/var/lib/秘密/身份.pem")
        );
        assert_eq!(targets.credential_owner, Some((123, 456)));
        assert_eq!(resolver.names, ["uid:underwrap-work", "gid:underwrap-work"]);
    }
}

#[test]
fn missing_backend_and_path_diagnostics_keep_filename_and_field_context() {
    for (text, expected) in [
        ("unrelated: {}", "no backend section"),
        ("backend: null", "no backend section"),
        (
            "backend: {url: 'https://backend.example'}",
            "backend.client_identity_file",
        ),
        (
            "backend: {url: 'https://backend.example', client_identity_file: null}",
            "backend.client_identity_file",
        ),
        ("backend: {client_identity_file: /out}", "url"),
        (
            "backend: {url: 'https://backend.example', typo: x}",
            "unknown field `typo`",
        ),
    ] {
        let (_tmp, cli) = config_cli(text);
        let mut resolver = resolver();
        let error = format!(
            "{:#}",
            resolve_targets_with(&cli, &mut resolver).unwrap_err()
        );
        assert!(error.contains("work.yaml"));
        assert!(error.contains(expected), "{error}");
        assert!(resolver.names.is_empty());
    }
}

#[test]
fn parsing_and_path_rejections_precede_generation_and_preserve_destination() {
    let tmp = tempfile::TempDir::new().unwrap();
    let identity = tmp.path().join("identity.pem");
    // A harmless marker, not credential material.
    std::fs::write(&identity, b"unchanged marker").unwrap();
    let config = tmp.path().join("work.yaml");
    let backend = format!(
        "backend: {{url: 'https://backend.example', client_identity_file: '{}'}}\n",
        identity.display()
    );
    let nested = format!("{}0{}", "[".repeat(150), "]".repeat(150));
    for text in [
        format!("{backend}{backend}"),
        format!("{backend}ignored: {{nested: {{same: a, same: b}}}}"),
        format!("{backend}ignored: &cycle [*cycle]"),
        format!("{backend}ignored: {nested}"),
        format!("{backend}---\n{backend}"),
        format!("{backend}backend_typo: ["),
        format!(
            "backend: {{url: [], client_identity_file: '{}'}}",
            identity.display()
        ),
        format!(
            "backend: {{url: 'https://backend.example', client_identity_file: '{}', client_identity_file: '{}'}}",
            identity.display(),
            identity.display()
        ),
        "backend: {<<: {url: 'https://backend.example', client_identity_file: /out}}".to_owned(),
        "backend: {url: 'https://backend.example', client_identity_file: relative/out}".to_owned(),
        format!(
            "backend: {{url: 'https://backend.example', client_identity_file: '{}/./identity.pem'}}",
            tmp.path().display()
        ),
    ] {
        std::fs::write(&config, &text).unwrap();
        let before = snapshot(&identity);
        let generated = Cell::new(false);
        let error = run_after_hardening(
            Cli {
                client_identity_out: None,
                instance_config: Some(config.clone()),
            },
            &mut resolver(),
            || {
                generated.set(true);
                bail!("generation must not run")
            },
        );
        assert!(error.is_err(), "accepted {text:?}");
        assert!(!generated.get());
        assert_eq!(snapshot(&identity), before);
    }
}

#[test]
fn ordinary_aliases_and_quoted_null_like_path_scalars_remain_explicit() {
    let (_tmp, cli) = config_cli("backend: {url: &text 'null', client_identity_file: *text}");
    let targets = resolve_targets_with(&cli, &mut resolver()).unwrap();
    assert_eq!(targets.client_identity_out, PathBuf::from("null"));
    assert!(validate_output_path(&targets.client_identity_out).is_err());
}
