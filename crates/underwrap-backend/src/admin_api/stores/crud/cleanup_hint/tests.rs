use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::Arc;

use hyper::StatusCode;
use underwrap_protocol::config::{StorageConfig, StoreRecord};

use super::for_directory;
use crate::store_registry::{StoreEntry, StoreState, load_store_records, save_store_record};
use crate::test_support::{admin_principal, crud_state, delete_store_for_test};

// The shell can reach only builtins/functions, never external rm. Both ordinary
// removal and synthetic substitution/extra-command probes are inert recorders.
fn record_arguments(command: &str) -> Output {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("glob-control"), []).unwrap();
    // Resolve the pinned shell before deliberately disabling child PATH lookup.
    let shell = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join("sh"))
        .find(|path| path.is_file())
        .expect("POSIX shell on test PATH");
    Command::new(shell)
        .args([
            "-c",
            &format!(
                "rm() {{ printf 'rm\\0'; printf '%s\\0' \"$@\"; }}; \
                 probe() {{ printf 'unexpected-execution\\n' >&2; }}; {command}"
            ),
        ])
        .env_clear()
        .env("PATH", "/nonexistent-underwrap-test-path")
        .current_dir(tmp.path())
        .output()
        .unwrap()
}

fn assert_literal_command(command: &str, expected: &str) {
    assert!(command.starts_with("rm -rf -- '"));
    let output = record_arguments(command);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        format!("rm\0-rf\0--\0{expected}\0").as_bytes(),
    );
}

#[test]
fn posix_shell_receives_exactly_one_literal_path_without_expansion() {
    for path in [
        "/ordinary/store_123",
        "/space containing/store",
        "/single'quote/and\"double",
        "/back\\slash/ending\\",
        "/$(probe)/`probe`/${HOME}/$((1+2))",
        "/semi; probe; #comment",
        "/pipe|probe&probe&&probe||probe",
        "/redirect<> (parentheses) {braces} ! tilde~",
        "/wildcards*?[abc]/glob-control",
        "/line\nbreak\tand\rcarriage",
        "/unicode/Grüße/日本語",
        "/-leading-option",
        "/file/.",
        "/symlink/./",
        "file/.",
        "symlink/./",
        "./directory/../file/.",
        "-relative option with 'quote' and $(probe)",
        "relative/*?[abc]/`probe`; probe; #",
    ] {
        let hint = for_directory(Path::new(path)).unwrap();
        let expected = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!(
                "{}/{path}",
                std::env::current_dir().unwrap().to_str().unwrap()
            )
        };
        assert_eq!(hint.data_dir, expected);
        assert_literal_command(&hint.command, &hint.data_dir);
    }
}

#[test]
fn recorder_detects_the_original_split_and_inert_extra_execution() {
    let split = record_arguments("rm -rf /space containing/store");
    assert_eq!(split.stdout, b"rm\0-rf\0/space\0containing/store\0");
    let expansion = record_arguments("rm -rf /$(probe); probe");
    assert_eq!(
        expansion.stderr,
        b"unexpected-execution\nunexpected-execution\n",
    );
}

fn record(dir: &Path) -> StoreRecord {
    StoreRecord {
        id: "store_cleanup".into(),
        name: "cleanup".into(),
        description: None,
        created_at: None,
        storage: StorageConfig::Encrypted { dir: dir.into() },
    }
}

#[test]
fn relative_registry_path_is_resolved_and_delete_preserves_backing_generations() {
    for plain in [true, false] {
        check_relative_record_deletion(plain);
    }
}

fn check_relative_record_deletion(plain: bool) {
    let cwd = std::env::current_dir().unwrap();
    let tmp = tempfile::TempDir::new_in(&cwd).unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let data = tmp.path().join("relative data 'quote' $(probe)");
    let generation = tmp
        .path()
        .join(".store_cleanup.underwrap-rotation-0123456789abcdef");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(generation.join("backup")).unwrap();
    std::fs::write(data.join("inert-marker"), []).unwrap();
    std::fs::write(generation.join("backup/inert-marker"), []).unwrap();
    let mut record = record(data.strip_prefix(&cwd).unwrap());
    if plain {
        record.storage = StorageConfig::Plain {
            dir: data.strip_prefix(&cwd).unwrap().into(),
        };
    }
    save_store_record(&state.admin().stores_dir, &record).unwrap();
    let loaded = load_store_records(&state.admin().stores_dir).unwrap();
    assert_eq!(loaded, HashMap::from([(record.id.clone(), record.clone())]));
    *state.shared.stores.write().unwrap() = crate::store_registry::StoreRegistry {
        entries: crate::store_registry::open_initial_stores(loaded).unwrap(),
    };
    let p = admin_principal();
    let (status, _) = delete_store_for_test(state.clone(), &p, &record.id, false);
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, body) = delete_store_for_test(state.clone(), &p, &record.id, true);
    assert_eq!(status, StatusCode::OK);
    let hint = &body["cleanup_hint"];
    assert_eq!(hint["data_dir"], data.to_str().unwrap());
    assert_literal_command(hint["command"].as_str().unwrap(), data.to_str().unwrap());
    assert!(
        load_store_records(&state.admin().stores_dir)
            .unwrap()
            .is_empty()
    );
    assert!(state.shared.stores.read().unwrap().entries.is_empty());
    assert!(data.join("inert-marker").is_file());
    assert!(generation.join("backup/inert-marker").is_file());
}

#[test]
fn locked_missing_directory_still_has_exact_guidance() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dir = tmp.path().join("missing 'directory'");
    assert!(!dir.exists());
    let hint = for_directory(&dir).unwrap();
    assert_literal_command(&hint.command, dir.to_str().unwrap());
    assert!(!dir.exists());
}

#[test]
fn unavailable_working_directory_preserves_registry_ownership() {
    const CHILD: &str = "UNDERWRAP_TEST_CLEANUP_MISSING_CWD";
    if std::env::var_os(CHILD).is_none() {
        // Working-directory mutation is isolated from every other test thread.
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "admin_api::stores::crud::cleanup_hint::tests::unavailable_working_directory_preserves_registry_ownership",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let record = record(Path::new("relative-store"));
    save_store_record(&state.admin().stores_dir, &record).unwrap();
    state.shared.stores.write().unwrap().entries.insert(
        record.id.clone(),
        StoreEntry::new(record.clone(), StoreState::Locked),
    );
    let cwd = tmp.path().join("empty-working-directory");
    std::fs::create_dir(&cwd).unwrap();
    std::env::set_current_dir(&cwd).unwrap();
    std::fs::remove_dir(&cwd).unwrap();
    assert!(std::env::current_dir().is_err());
    let (status, body) = delete_store_for_test(state.clone(), &admin_principal(), &record.id, true);
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "cleanup_path_unrepresentable");
    assert_eq!(
        state
            .shared
            .stores
            .read()
            .unwrap()
            .entry(&record.id)
            .unwrap()
            .record,
        record
    );
    assert_eq!(
        load_store_records(&state.admin().stores_dir).unwrap(),
        HashMap::from([(record.id.clone(), record)])
    );
}

#[test]
fn unrepresentable_guidance_preserves_registry_ownership() {
    use std::os::unix::ffi::OsStringExt;

    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let persisted = record(&tmp.path().join("persisted"));
    save_store_record(&state.admin().stores_dir, &persisted).unwrap();
    for invalid in [
        std::ffi::OsString::from_vec(b"/invalid-utf8-\xff".to_vec()),
        std::ffi::OsString::from("/invalid\0nul"),
        std::ffi::OsString::new(),
    ] {
        let live = record(Path::new(&invalid));
        state.shared.stores.write().unwrap().entries.insert(
            live.id.clone(),
            StoreEntry::new(live.clone(), StoreState::Locked),
        );
        let (status, body) =
            delete_store_for_test(state.clone(), &admin_principal(), &live.id, true);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body,
            serde_json::json!({
                "code": "cleanup_path_unrepresentable",
                "message": "could not prepare exact cleanup guidance; store registry record was not removed",
            })
        );
        assert_eq!(
            state
                .shared
                .stores
                .read()
                .unwrap()
                .entry(&live.id)
                .unwrap()
                .record,
            live,
        );
        assert_eq!(
            load_store_records(&state.admin().stores_dir).unwrap(),
            HashMap::from([(persisted.id.clone(), persisted.clone())]),
        );
    }
}
