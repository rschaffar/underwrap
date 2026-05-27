use super::*;
use std::path::PathBuf;
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

fn find_in_path(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(bin))
            .find(|p| p.is_file())
    })
}

/// `apply_helper_env` must clear the inherited (caller-derived) environment
/// and reintroduce only the trusted whitelist, so a caller-controlled
/// service-discovery variable like `XDG_DATA_DIRS` cannot reach a helper.
#[test]
fn apply_helper_env_clears_inherited_and_keeps_whitelist() {
    let _guard = env_lock().lock().unwrap();

    let Some(env_bin) = find_in_path("env") else {
        eprintln!("skipping: no `env` binary on PATH");
        return;
    };

    // Pollute the process environment with a caller-controlled variable; it
    // must not survive into the helper command.
    unsafe {
        std::env::set_var("XDG_DATA_DIRS", "/tmp/attacker-controlled");
    }

    let mut helper_env = HashMap::new();
    helper_env.insert("HOME".to_string(), "/home/trusted".to_string());
    helper_env.insert("PATH".to_string(), "/run/current-system/sw/bin".to_string());

    let mut cmd = Command::new(&env_bin);
    apply_helper_env(&mut cmd, &helper_env);
    let output = cmd.output().expect("run env binary");

    unsafe {
        std::env::remove_var("XDG_DATA_DIRS");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("XDG_DATA_DIRS"),
        "caller XDG_DATA_DIRS leaked into helper env:\n{stdout}"
    );
    assert!(
        stdout.contains("HOME=/home/trusted"),
        "expected whitelisted HOME in helper env:\n{stdout}"
    );
    assert!(
        stdout.contains("PATH=/run/current-system/sw/bin"),
        "expected whitelisted PATH in helper env:\n{stdout}"
    );
}
