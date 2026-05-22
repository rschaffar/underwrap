use super::*;
use crate::users;

#[test]
fn parse_backend_config_parses_minimal_yaml() {
    // Empty YAML works because the backend-managed store registry
    // path and https+admin blocks are optional.
    let config = parse_backend_config("{}\n").unwrap();
    assert!(config.stores_file.is_none());
    assert!(config.client_instances_file.is_none());
    assert!(config.https.is_none());
    assert!(config.admin.is_none());
}

#[test]
fn parse_backend_config_accepts_dir_overrides() {
    let config = parse_backend_config(
        r#"
stores_file: /var/u/stores.json
client_instances_file: /var/u/client-instances.json
users_file: /var/u/users.json
"#,
    )
    .unwrap();
    assert_eq!(
        config.stores_file(),
        std::path::PathBuf::from("/var/u/stores.json"),
    );
    assert_eq!(
        config.client_instances_file(),
        std::path::PathBuf::from("/var/u/client-instances.json"),
    );
    assert_eq!(
        config.users_file(),
        std::path::PathBuf::from("/var/u/users.json"),
    );
}

#[test]
fn recover_user_writes_user_to_empty_state_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let users_file = dir.path().join("users.json");

    recover_user(&users_file, "Alice", "hunter2").unwrap();

    // Read it back: user landed in the state file with a verifiable
    // password.
    let coll = users::load(&users_file).unwrap();
    let u = coll.find_by_username("alice").unwrap();
    assert!(crate::auth::verify_password(&u.password_hash, "hunter2"));
    // Sentinel is NOT set — recovery deliberately stays "offline".
    assert!(!users::bootstrap_completed(&users_file).unwrap());
}

#[test]
fn recover_user_refuses_existing_username() {
    let dir = tempfile::TempDir::new().unwrap();
    let users_file = dir.path().join("users.json");
    recover_user(&users_file, "alice", "p1").unwrap();
    let err = recover_user(&users_file, "Alice", "p2").unwrap_err();
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn recover_user_rejects_empty_password() {
    let dir = tempfile::TempDir::new().unwrap();
    let users_file = dir.path().join("users.json");
    let err = recover_user(&users_file, "alice", "").unwrap_err();
    assert!(err.to_string().contains("password"));
}
