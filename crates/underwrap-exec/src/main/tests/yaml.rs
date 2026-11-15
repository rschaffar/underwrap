use super::*;

#[test]
fn generated_json_and_quoted_yaml_preserve_helper_paths_and_defaults() {
    let config = parse_instance_config_text(
        r#"{"socket":"/run/秘密/socket","ns_binary":"null","dbus_daemon_binary":null,"secretsd_binary":"~"}"#,
    )
    .unwrap();
    assert_eq!(config.socket, "/run/秘密/socket");
    assert_eq!(config.ns_binary.as_deref(), Some("null"));
    assert_eq!(config.dbus_daemon_binary, None);
    assert_eq!(config.secretsd_binary.as_deref(), Some("~"));
    assert_eq!(
        parse_instance_config_text(
            "socket: /run/秘密/socket\nns_binary: 'null'\ndbus_daemon_binary: null\nsecretsd_binary: '~'\n"
        )
        .unwrap(),
        config
    );
}

#[test]
fn rejects_ambiguous_or_invalid_privileged_sidecars() {
    for text in [
        "{}",
        "socket: []",
        "socket: null",
        "socket: /run/one\nsocket: /run/two",
        "socket: /run/one\nns_binary: /bin/one\nns_binary: /bin/two",
        "socket: /run/one\nunknown: x",
        "socket: /run/one\n---\nsocket: /run/two",
        "base: &base {socket: /run/one}\n<<: *base",
        "socket: &cycle [*cycle]",
    ] {
        assert!(
            parse_instance_config_text(text).is_err(),
            "accepted {text:?}"
        );
    }
}

#[test]
fn ordinary_aliases_do_not_enable_merge_expansion() {
    let config = parse_instance_config_text("socket: &path /run/one\nns_binary: *path").unwrap();
    assert_eq!(config.ns_binary.as_deref(), Some("/run/one"));
    let error = parse_instance_config_text("<<: {socket: /run/one}").unwrap_err();
    assert!(format!("{error:#}").contains("unknown field `<<`"));
}
