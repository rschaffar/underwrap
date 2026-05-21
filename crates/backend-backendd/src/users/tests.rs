use super::*;

fn sample_record(username: &str) -> UserRecord {
    UserRecord {
        id: new_user_id(),
        username: username.to_string(),
        password_hash:
            "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
                .to_string(),
        created_at: 1000,
        password_changed_at: 1000,
    }
}

fn test_state_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("underwrap-user-state-test-{}.json", new_user_id()))
}

#[test]
fn collection_round_trips_through_serde() {
    let mut c = UserCollection::default();
    c.add(sample_record("alice")).unwrap();
    c.add(sample_record("bob")).unwrap();
    let bytes = serde_json::to_vec(&c).unwrap();
    let parsed: UserCollection = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed, c);
}

#[test]
fn add_rejects_duplicate_username_case_insensitively() {
    let mut c = UserCollection::default();
    c.add(sample_record("Alice")).unwrap();
    let err = c.add(sample_record("alice")).unwrap_err();
    assert!(err.to_string().contains("already exists"));
    assert_eq!(c.users[0].username, "alice");
}

#[test]
fn add_rejects_empty_username() {
    let mut c = UserCollection::default();
    let err = c.add(sample_record("   ")).unwrap_err();
    assert!(err.to_string().contains("non-empty"));
}

#[test]
fn add_rejects_non_argon2_hash() {
    let mut c = UserCollection::default();
    let mut bad = sample_record("alice");
    bad.password_hash = "plaintext".to_string();
    let err = c.add(bad).unwrap_err();
    assert!(err.to_string().contains("argon2"));
}

#[test]
fn find_by_username_is_case_insensitive() {
    let mut c = UserCollection::default();
    c.add(sample_record("Alice")).unwrap();
    assert!(c.find_by_username("alice").is_some());
    assert!(c.find_by_username("ALICE").is_some());
    assert!(c.find_by_username("  alice  ").is_some());
    assert!(c.find_by_username("bob").is_none());
}

#[test]
fn empty_collection_signals_bootstrap_required() {
    let path = test_state_path();
    let collection = load(&path).unwrap();
    assert!(collection.is_empty(), "fresh state yields empty collection");
}

#[test]
fn save_and_load_round_trip() {
    let path = test_state_path();
    let mut c = UserCollection::default();
    c.add(sample_record("alice")).unwrap();
    save(&path, &c).unwrap();
    let loaded = load(&path).unwrap();
    assert_eq!(loaded, c);
}

#[test]
fn load_rejects_unknown_state_file_version() {
    let path = test_state_path();
    std::fs::write(&path, r#"{"version":99,"users":[]}"#).unwrap();
    let err = load(&path).unwrap_err();
    assert!(err.to_string().contains("version"));
}

#[test]
fn bootstrap_sentinel_absent_by_default() {
    let path = test_state_path();
    assert!(!bootstrap_completed(&path).unwrap());
    assert!(load_bootstrap_sentinel(&path).unwrap().is_none());
}

#[test]
fn bootstrap_sentinel_round_trip() {
    let path = test_state_path();
    let sentinel = BootstrapSentinel {
        completed_at: 1234,
        by_username: "alice".into(),
    };
    mark_bootstrap_completed(&path, &sentinel).unwrap();
    assert!(bootstrap_completed(&path).unwrap());
    let loaded = load_bootstrap_sentinel(&path).unwrap().unwrap();
    assert_eq!(loaded, sentinel);
}

#[test]
fn saving_collection_preserves_existing_bootstrap_sentinel() {
    let path = test_state_path();
    let sentinel = BootstrapSentinel {
        completed_at: 1234,
        by_username: "alice".into(),
    };
    mark_bootstrap_completed(&path, &sentinel).unwrap();
    let mut c = UserCollection::default();
    c.add(sample_record("alice")).unwrap();
    save(&path, &c).unwrap();
    assert_eq!(load(&path).unwrap(), c);
    assert_eq!(load_bootstrap_sentinel(&path).unwrap().unwrap(), sentinel);
}

#[test]
fn remove_refuses_last_user() {
    let mut c = UserCollection::default();
    c.add(sample_record("alice")).unwrap();
    let id = c.users[0].id.clone();
    let err = c.remove(&id).unwrap_err();
    assert_eq!(err.to_string(), "last_user_protected");
    assert_eq!(c.len(), 1, "the only user must still be present");
}

#[test]
fn remove_succeeds_when_more_than_one_user_exists() {
    let mut c = UserCollection::default();
    c.add(sample_record("alice")).unwrap();
    c.add(sample_record("bob")).unwrap();
    let bob_id = c.users[1].id.clone();
    let removed = c.remove(&bob_id).unwrap();
    assert_eq!(removed.username, "bob");
    assert_eq!(c.len(), 1);
}

#[test]
fn debug_redacts_password_hash() {
    let r = sample_record("alice");
    let dbg = format!("{r:?}");
    assert!(
        !dbg.contains("argon2"),
        "Debug must not leak the PHC string"
    );
    assert!(dbg.contains("<redacted>"));
    let mut c = UserCollection::default();
    c.add(r).unwrap();
    let dbg = format!("{c:?}");
    assert!(!dbg.contains("argon2"));
}

#[test]
fn load_rejects_v1_state_file() {
    let path = test_state_path();
    std::fs::write(&path, r#"{"version":1,"operators":[]}"#).unwrap();
    let err = load(&path).unwrap_err();
    assert!(
        err.to_string()
            .contains("unsupported user-state file version")
    );
}
