use super::*;
use crate::users;

#[test]
fn parse_backend_config_parses_minimal_yaml() {
    // Empty YAML works because the backend-managed store registry
    // path and https+admin blocks are optional.
    let config = parse_backend_config("{}\n").unwrap();
    assert!(config.stores_dir.is_none());
    assert!(config.clients_dir.is_none());
    assert!(config.users_dir.is_none());
    assert!(config.bootstrap_file.is_none());
    assert!(config.https.is_none());
    assert!(config.admin.is_none());
}

#[test]
fn parse_backend_config_accepts_dir_overrides() {
    let config = parse_backend_config(
        r#"
stores_dir: /var/u/stores
clients_dir: /var/u/clients
users_dir: /var/u/users
bootstrap_file: /var/u/bootstrap.json
"#,
    )
    .unwrap();
    assert_eq!(
        config.stores_dir(),
        std::path::PathBuf::from("/var/u/stores"),
    );
    assert_eq!(
        config.clients_dir(),
        std::path::PathBuf::from("/var/u/clients"),
    );
    assert_eq!(config.users_dir(), std::path::PathBuf::from("/var/u/users"),);
    assert_eq!(
        config.bootstrap_file(),
        std::path::PathBuf::from("/var/u/bootstrap.json"),
    );
}

#[test]
fn recover_user_writes_user_to_empty_state_dir() {
    let dir = tempfile::TempDir::new().unwrap();
    let users_dir = dir.path().join("users");
    let bootstrap_file = dir.path().join("bootstrap.json");

    recover_user(&users_dir, "Alice", "hunter2").unwrap();

    // Read it back: user landed in the dir with a verifiable
    // password.
    let coll = users::load(&users_dir).unwrap();
    let u = coll.find_by_username("alice").unwrap();
    assert!(crate::auth::verify_password(&u.password_hash, "hunter2"));
    // Sentinel is NOT set — recovery deliberately stays "offline".
    assert!(!users::bootstrap_completed(&bootstrap_file).unwrap());
}

#[test]
fn recover_user_refuses_existing_username() {
    let dir = tempfile::TempDir::new().unwrap();
    let users_dir = dir.path().join("users");
    recover_user(&users_dir, "alice", "p1").unwrap();
    let err = recover_user(&users_dir, "Alice", "p2").unwrap_err();
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn recover_user_rejects_empty_password() {
    let dir = tempfile::TempDir::new().unwrap();
    let users_dir = dir.path().join("users");
    let err = recover_user(&users_dir, "alice", "").unwrap_err();
    assert!(err.to_string().contains("password"));
}
