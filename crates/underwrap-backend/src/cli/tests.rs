use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::sync::{Arc, Barrier};

#[derive(Debug, PartialEq, Eq)]
struct FileSnapshot {
    bytes: Vec<u8>,
    device: u64,
    inode: u64,
    mode: u32,
    length: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

fn snapshot_file(path: &Path) -> FileSnapshot {
    let metadata = std::fs::symlink_metadata(path).unwrap();
    FileSnapshot {
        bytes: std::fs::read(path).unwrap(),
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.permissions().mode(),
        length: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
    }
}

use super::*;
use crate::users;

#[test]
fn parse_backend_config_parses_minimal_yaml() {
    let config = parse_backend_config("{}\n").unwrap();
    assert_eq!(
        config.stores_dir,
        std::path::PathBuf::from(crate::config::DEFAULT_STORES_DIR),
    );
    assert_eq!(
        config.clients_dir,
        std::path::PathBuf::from(crate::config::DEFAULT_CLIENTS_DIR),
    );
    assert_eq!(
        config.users_dir,
        std::path::PathBuf::from(crate::users::DEFAULT_USERS_DIR),
    );
    assert_eq!(
        config.bootstrap_file,
        std::path::PathBuf::from(crate::users::DEFAULT_BOOTSTRAP_FILE),
    );
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
    assert_eq!(config.stores_dir, std::path::PathBuf::from("/var/u/stores"));
    assert_eq!(
        config.clients_dir,
        std::path::PathBuf::from("/var/u/clients"),
    );
    assert_eq!(config.users_dir, std::path::PathBuf::from("/var/u/users"));
    assert_eq!(
        config.bootstrap_file,
        std::path::PathBuf::from("/var/u/bootstrap.json"),
    );
}

fn bootstrap_paths() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::TempDir::new().unwrap();
    let users_dir = tmp.path().join("users");
    let bootstrap_file = tmp.path().join("bootstrap.json");
    (tmp, users_dir, bootstrap_file)
}

#[test]
fn bootstrap_user_creates_normalized_operator_and_sentinel() {
    let (_tmp, users_dir, bootstrap_file) = bootstrap_paths();
    bootstrap_user(&users_dir, &bootstrap_file, " Alice ", "hunter2-long").unwrap();

    let collection = users::load(&users_dir).unwrap();
    let user = collection.find_by_username("alice").unwrap();
    assert!(crate::auth::verify_password(
        &user.password_hash,
        "hunter2-long"
    ));
    assert!(users::bootstrap_completed(&bootstrap_file).unwrap());
    assert!(bootstrap_lock_path(&bootstrap_file).unwrap().is_file());
}

#[test]
fn bootstrap_user_enforces_username_and_password_policy() {
    let (_tmp, users_dir, bootstrap_file) = bootstrap_paths();
    assert!(bootstrap_user(&users_dir, &bootstrap_file, " ", "hunter2-long").is_err());
    assert!(bootstrap_user(&users_dir, &bootstrap_file, "alice", "").is_err());
    let short = "a".repeat(crate::auth::MIN_PASSWORD_LEN - 1);
    assert!(bootstrap_user(&users_dir, &bootstrap_file, "alice", &short).is_err());
    assert!(users::load(&users_dir).unwrap().is_empty());
    assert!(!users::bootstrap_completed(&bootstrap_file).unwrap());
}

#[test]
fn bootstrap_user_refuses_existing_user_or_sentinel() {
    let (_tmp, users_dir, bootstrap_file) = bootstrap_paths();
    recover_user(&users_dir, "rescuer", "recovery-password").unwrap();
    assert!(
        bootstrap_user(&users_dir, &bootstrap_file, "alice", "hunter2-long")
            .unwrap_err()
            .to_string()
            .contains("operator already exists")
    );
    assert_eq!(users::load(&users_dir).unwrap().len(), 1);
    assert!(!users::bootstrap_completed(&bootstrap_file).unwrap());

    let (_tmp, users_dir, bootstrap_file) = bootstrap_paths();
    users::mark_bootstrap_completed(&bootstrap_file).unwrap();
    assert!(
        bootstrap_user(&users_dir, &bootstrap_file, "alice", "hunter2-long")
            .unwrap_err()
            .to_string()
            .contains("already been completed")
    );
    assert!(users::load(&users_dir).unwrap().is_empty());
}

#[test]
fn bootstrap_user_rejects_malformed_collection() {
    let (_tmp, users_dir, bootstrap_file) = bootstrap_paths();
    std::fs::create_dir_all(&users_dir).unwrap();
    std::fs::write(users_dir.join("broken.json"), b"not-json").unwrap();
    assert!(bootstrap_user(&users_dir, &bootstrap_file, "alice", "hunter2-long").is_err());
    assert!(!users::bootstrap_completed(&bootstrap_file).unwrap());
}

#[test]
fn bootstrap_user_rejects_dangling_sentinel_symlink_as_completed() {
    let (_tmp, users_dir, bootstrap_file) = bootstrap_paths();
    symlink("missing-sentinel-target", &bootstrap_file).unwrap();

    assert!(users::bootstrap_completed(&bootstrap_file).unwrap());
    let error = bootstrap_user(&users_dir, &bootstrap_file, "alice", "hunter2-long")
        .unwrap_err()
        .to_string();

    assert!(error.contains("already been completed"));
    assert!(
        std::fs::symlink_metadata(&bootstrap_file)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(users::load(&users_dir).unwrap().is_empty());
}

#[test]
fn bootstrap_lock_rejects_symlink_and_sets_close_on_exec() {
    let (_tmp, users_dir, bootstrap_file) = bootstrap_paths();
    let lock_path = bootstrap_lock_path(&bootstrap_file).unwrap();
    symlink("missing-lock-target", &lock_path).unwrap();

    let error = bootstrap_user(&users_dir, &bootstrap_file, "alice", "hunter2-long")
        .unwrap_err()
        .to_string();
    assert!(error.contains("opening lock file"));
    assert!(users::load(&users_dir).unwrap().is_empty());
    assert!(!users::bootstrap_completed(&bootstrap_file).unwrap());

    std::fs::remove_file(&lock_path).unwrap();
    let lock = acquire_bootstrap_lock(&bootstrap_file).unwrap();
    // SAFETY: the lock owns this live descriptor for the entire call, and
    // `F_GETFD` neither follows pointers nor changes descriptor ownership.
    let descriptor_flags = unsafe { libc::fcntl(lock._file.as_raw_fd(), libc::F_GETFD) };
    assert!(
        descriptor_flags >= 0,
        "F_GETFD failed: {}",
        std::io::Error::last_os_error()
    );
    assert_ne!(descriptor_flags & libc::FD_CLOEXEC, 0);
}

#[test]
fn bootstrap_user_repeat_preserves_authentication_and_unrelated_state() {
    let (tmp, users_dir, bootstrap_file) = bootstrap_paths();
    let store_marker = tmp.path().join("stores/store-marker");
    let client_marker = tmp.path().join("clients/client-marker");
    std::fs::create_dir_all(store_marker.parent().unwrap()).unwrap();
    std::fs::create_dir_all(client_marker.parent().unwrap()).unwrap();
    std::fs::write(&store_marker, b"store-state").unwrap();
    std::fs::write(&client_marker, b"client-state").unwrap();

    bootstrap_user(&users_dir, &bootstrap_file, "alice", "hunter2-long").unwrap();
    let original = users::load(&users_dir)
        .unwrap()
        .find_by_username("alice")
        .unwrap()
        .clone();
    let user_path = users_dir.join(format!("{}.json", original.id));
    let user_before = snapshot_file(&user_path);
    let sentinel_before = snapshot_file(&bootstrap_file);
    let store_before = snapshot_file(&store_marker);
    let client_before = snapshot_file(&client_marker);

    assert!(bootstrap_user(&users_dir, &bootstrap_file, "eve", "another-password").is_err());

    let after = users::load(&users_dir).unwrap();
    let winner = after.find_by_username("alice").unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(winner.password_hash, original.password_hash);
    assert_eq!(winner.created_at, original.created_at);
    assert_eq!(winner.password_changed_at, original.password_changed_at);
    assert_eq!(snapshot_file(&user_path), user_before);
    assert_eq!(snapshot_file(&bootstrap_file), sentinel_before);
    assert_eq!(snapshot_file(&store_marker), store_before);
    assert_eq!(snapshot_file(&client_marker), client_before);
}

#[test]
fn bootstrap_user_serializes_concurrent_callers() {
    let (_tmp, users_dir, bootstrap_file) = bootstrap_paths();
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for username in ["alice", "bob"] {
        let users_dir = users_dir.clone();
        let bootstrap_file = bootstrap_file.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            bootstrap_user(&users_dir, &bootstrap_file, username, "hunter2-long")
        }));
    }
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(users::load(&users_dir).unwrap().len(), 1);
    assert!(users::bootstrap_completed(&bootstrap_file).unwrap());

    // A new descriptor can acquire the retained lock inode after both command
    // owners exit, proving ordinary close does not leave stale lock state.
    drop(acquire_bootstrap_lock(&bootstrap_file).unwrap());
}

#[test]
fn bootstrap_user_does_not_mark_sentinel_when_user_save_fails() {
    let (_tmp, users_dir, bootstrap_file) = bootstrap_paths();
    let marker_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let marker_called_clone = Arc::clone(&marker_called);
    let error = bootstrap_user_with(
        &users_dir,
        &bootstrap_file,
        "alice",
        "hunter2-long",
        |_dir, _record| anyhow::bail!("injected user write failure"),
        move |_path| {
            marker_called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("persisting initial operator"));
    assert!(!marker_called.load(std::sync::atomic::Ordering::SeqCst));
    assert!(users::load(&users_dir).unwrap().is_empty());
    assert!(!users::bootstrap_completed(&bootstrap_file).unwrap());
}

#[test]
fn bootstrap_user_preserves_user_when_sentinel_save_fails() {
    let (_tmp, users_dir, bootstrap_file) = bootstrap_paths();
    let error = bootstrap_user_with(
        &users_dir,
        &bootstrap_file,
        "alice",
        "hunter2-long",
        users::save_user,
        |_path| anyhow::bail!("injected sentinel write failure"),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("operator remains available for login")
    );
    assert!(!users::bootstrap_completed(&bootstrap_file).unwrap());
    let collection = users::load(&users_dir).unwrap();
    let user = collection.find_by_username("alice").unwrap();
    assert!(crate::auth::verify_password(
        &user.password_hash,
        "hunter2-long"
    ));
    assert!(bootstrap_user(&users_dir, &bootstrap_file, "bob", "another-password").is_err());
    assert_eq!(users::load(&users_dir).unwrap().len(), 1);
}

#[test]
fn recover_user_writes_user_to_empty_state_dir() {
    let dir = tempfile::TempDir::new().unwrap();
    let users_dir = dir.path().join("users");
    let bootstrap_file = dir.path().join("bootstrap.json");

    recover_user(&users_dir, "Alice", "hunter2").unwrap();

    let coll = users::load(&users_dir).unwrap();
    let u = coll.find_by_username("alice").unwrap();
    assert!(crate::auth::verify_password(&u.password_hash, "hunter2"));
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
