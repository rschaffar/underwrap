use super::*;

#[test]
fn wrapper_builder_creates_env_projection() {
    let wrapper = wrapper("/usr/bin/gh", "github/token", "GH_TOKEN");
    assert_eq!(wrapper.target, PathBuf::from("/usr/bin/gh"));
    assert_eq!(wrapper.resource, "github/token");
    assert!(wrapper.env);
    assert!(wrapper.files.is_none());
    assert!(!wrapper.libsecret);
}

#[test]
fn files_wrapper_builder_sets_path() {
    let w = files_wrapper("/usr/bin/true", "gws/state", ".config/gws");
    assert_eq!(w.resource, "gws/state");
    assert_eq!(
        w.files.as_ref().unwrap().paths,
        vec![".config/gws".to_string()]
    );
}

#[test]
fn files_wrapper_paths_builder_carries_all_paths() {
    let w = files_wrapper_paths(
        "/usr/bin/true",
        "gws/state",
        &[".config/gws", ".local/share/gws"],
    );
    assert_eq!(
        w.files.unwrap().paths,
        vec![".config/gws".to_string(), ".local/share/gws".to_string()],
    );
}
