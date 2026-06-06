use super::*;

const HOME: &str = "/home/alice";

fn home() -> PathBuf {
    PathBuf::from(HOME)
}

#[test]
fn tilde_resolves_to_home() {
    assert_eq!(resolve_spec("~", &home()).unwrap(), home());
}

#[test]
fn tilde_rel_resolves_under_home() {
    assert_eq!(
        resolve_spec("~/.config/gh", &home()).unwrap(),
        home().join(".config/gh")
    );
}

#[test]
fn absolute_path_passes_through() {
    assert_eq!(
        resolve_spec("/srv/data", &home()).unwrap(),
        PathBuf::from("/srv/data")
    );
}

#[test]
fn relative_non_tilde_is_rejected() {
    assert!(resolve_spec(".config/gh", &home()).is_err());
}

#[test]
fn tilde_user_form_is_rejected() {
    assert!(resolve_spec("~bob/x", &home()).is_err());
}

#[test]
fn empty_is_rejected() {
    assert!(resolve_spec("", &home()).is_err());
}

#[test]
fn parent_dir_traversal_is_rejected() {
    assert!(resolve_spec("~/../etc", &home()).is_err());
    assert!(resolve_spec("/srv/../etc/shadow", &home()).is_err());
}
