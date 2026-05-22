use super::*;
use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn require_absolute_env_path_accepts_absolute_paths() {
    let _guard = env_lock().lock().unwrap();
    unsafe {
        std::env::set_var("UNDERWRAP_TEST_BIN", "/run/current-system/sw/bin/test-bin");
    }

    let path = require_absolute_env_path("UNDERWRAP_TEST_BIN").unwrap();
    assert_eq!(path, "/run/current-system/sw/bin/test-bin");

    unsafe {
        std::env::remove_var("UNDERWRAP_TEST_BIN");
    }
}

#[test]
fn require_absolute_env_path_rejects_missing_and_relative_paths() {
    let _guard = env_lock().lock().unwrap();
    unsafe {
        std::env::remove_var("UNDERWRAP_TEST_BIN");
    }
    let err = require_absolute_env_path("UNDERWRAP_TEST_BIN").unwrap_err();
    assert!(err.contains("not set"));

    unsafe {
        std::env::set_var("UNDERWRAP_TEST_BIN", "relative-bin");
    }
    let err = require_absolute_env_path("UNDERWRAP_TEST_BIN").unwrap_err();
    assert!(err.contains("must be an absolute path"));

    unsafe {
        std::env::remove_var("UNDERWRAP_TEST_BIN");
    }
}
