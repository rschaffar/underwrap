use super::*;

#[test]
fn shipped_backend_config_example_parses() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("backend crate lives under <repo>/crates");
    let path = repo_root.join("examples/backend.yaml");
    let yaml = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read shipped example {}: {error}", path.display()));

    let config: BackendConfig = crate::cli::parse_backend_config(&yaml)
        .unwrap_or_else(|error| panic!("{} does not parse: {error}", path.display()));
    assert!(config.https.is_some(), "example must enable HTTPS");
    assert!(config.admin.is_some(), "example must enable the admin UI");
}
