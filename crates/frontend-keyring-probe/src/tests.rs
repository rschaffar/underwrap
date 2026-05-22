use super::*;

#[test]
fn resolve_secret_accepts_explicit_secret() {
    let cmd = SecretCommand {
        service: "gh:github.com".to_string(),
        user: "alice".to_string(),
        secret: Some("topsecret".to_string()),
        secret_stdin: false,
    };
    assert_eq!(cmd.resolve_secret().unwrap(), "topsecret");
}

#[test]
fn resolve_secret_rejects_missing_or_conflicting_sources() {
    let missing = SecretCommand {
        service: "svc".to_string(),
        user: "user".to_string(),
        secret: None,
        secret_stdin: false,
    };
    assert!(missing.resolve_secret().is_err());

    let conflicting = SecretCommand {
        service: "svc".to_string(),
        user: "user".to_string(),
        secret: Some("x".to_string()),
        secret_stdin: true,
    };
    assert!(conflicting.resolve_secret().is_err());
}

#[test]
fn attributes_match_go_keyring_shape() {
    let attrs = attributes("gh:github.com", "rschaffar");
    assert_eq!(attrs.get("service"), Some(&"gh:github.com".to_string()));
    assert_eq!(attrs.get("username"), Some(&"rschaffar".to_string()));
}
