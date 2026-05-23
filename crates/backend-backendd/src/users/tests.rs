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

fn test_users_dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("underwrap-users-dir-test-{}", new_user_id()))
}

fn test_bootstrap_file() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("underwrap-bootstrap-test-{}.json", new_user_id()))
}

#[test]
fn add_rejects_duplicate_username_case_insensitively() {
    let mut c = UserCollection::default();
    c.add(sample_record("Alice")).unwrap();
    let err = c.add(sample_record("alice")).unwrap_err();
    assert!(matches!(err, UserMutationError::DuplicateUsername(_)));
    assert_eq!(c.users[0].username, "alice");
}

#[test]
fn add_rejects_empty_username() {
    let mut c = UserCollection::default();
    let err = c.add(sample_record("   ")).unwrap_err();
    assert!(matches!(err, UserMutationError::InvalidUsername));
}

#[test]
fn add_rejects_non_argon2_hash() {
    let mut c = UserCollection::default();
    let mut bad = sample_record("alice");
    bad.password_hash = "plaintext".to_string();
    let err = c.add(bad).unwrap_err();
    assert!(matches!(err, UserMutationError::BadHash));
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
fn empty_dir_signals_bootstrap_required() {
    let dir = test_users_dir();
    let collection = load(&dir).unwrap();
    assert!(collection.is_empty(), "fresh state yields empty collection");
}

#[test]
fn save_and_load_round_trip() {
    let dir = test_users_dir();
    let mut c = UserCollection::default();
    let a = sample_record("alice");
    let b = sample_record("bob");
    c.add(a.clone()).unwrap();
    c.add(b.clone()).unwrap();
    save_user(&dir, &a).unwrap();
    save_user(&dir, &b).unwrap();
    let loaded = load(&dir).unwrap();
    // Sort both sides by id so we don't depend on filesystem readdir
    // order.
    let mut got = loaded.users.clone();
    got.sort_by(|x, y| x.id.cmp(&y.id));
    let mut want = c.users.clone();
    want.sort_by(|x, y| x.id.cmp(&y.id));
    assert_eq!(got, want);
}

#[test]
fn id_lives_in_filename_not_body() {
    // The on-disk JSON must not carry the id — the file stem is the
    // sole source of truth.
    let dir = test_users_dir();
    let record = sample_record("alice");
    let id = record.id.clone();
    save_user(&dir, &record).unwrap();
    let text = std::fs::read_to_string(dir.join(format!("{id}.json"))).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    let obj = parsed.as_object().unwrap();
    assert!(
        !obj.contains_key("id"),
        "on-disk user JSON must not contain an 'id' key: {obj:?}",
    );
}

#[test]
fn delete_user_removes_file_and_is_idempotent() {
    let dir = test_users_dir();
    let record = sample_record("alice");
    let id = record.id.clone();
    save_user(&dir, &record).unwrap();
    assert!(dir.join(format!("{id}.json")).exists());
    delete_user(&dir, &id).unwrap();
    assert!(!dir.join(format!("{id}.json")).exists());
    // Second delete is a no-op (idempotent under retry).
    delete_user(&dir, &id).unwrap();
}

#[test]
fn duplicate_username_across_files_is_rejected_on_load() {
    let dir = test_users_dir();
    let a = sample_record("alice");
    let mut b = sample_record("ALICE");
    // Force the case-collision: the loader normalises username during
    // file parse only via the in-memory `add()`; here we bypass `add`
    // and write the file directly to simulate filesystem tampering.
    b.username = "alice".into();
    save_user(&dir, &a).unwrap();
    save_user(&dir, &b).unwrap();
    let err = load(&dir).expect_err("duplicate usernames must fail");
    assert!(format!("{err:#}").contains("duplicate username"));
}

#[test]
fn non_json_files_are_ignored() {
    let dir = test_users_dir();
    let record = sample_record("alice");
    let id = record.id.clone();
    save_user(&dir, &record).unwrap();
    std::fs::write(dir.join(format!("{id}.json.bak")), b"junk").unwrap();
    std::fs::write(dir.join("README"), b"notes").unwrap();
    let loaded = load(&dir).unwrap();
    assert_eq!(loaded.len(), 1);
}

#[test]
fn bootstrap_sentinel_absent_by_default() {
    let path = test_bootstrap_file();
    assert!(!bootstrap_completed(&path).unwrap());
    assert!(load_bootstrap_sentinel(&path).unwrap().is_none());
}

#[test]
fn bootstrap_sentinel_round_trip() {
    let path = test_bootstrap_file();
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
fn bootstrap_sentinel_lives_independently_of_users_dir() {
    // Writing the sentinel file must not affect the users dir, and
    // writing users must not affect the sentinel. They are siblings,
    // not one merged document any more.
    let dir = test_users_dir();
    let sentinel_path = test_bootstrap_file();
    let record = sample_record("alice");
    save_user(&dir, &record).unwrap();
    mark_bootstrap_completed(
        &sentinel_path,
        &BootstrapSentinel {
            completed_at: 1234,
            by_username: "alice".into(),
        },
    )
    .unwrap();
    assert_eq!(load(&dir).unwrap().len(), 1);
    assert!(bootstrap_completed(&sentinel_path).unwrap());
}

#[test]
fn remove_refuses_last_user() {
    let mut c = UserCollection::default();
    c.add(sample_record("alice")).unwrap();
    let id = c.users[0].id.clone();
    let err = c.remove(&id).unwrap_err();
    assert!(matches!(err, UserMutationError::LastUserProtected));
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
