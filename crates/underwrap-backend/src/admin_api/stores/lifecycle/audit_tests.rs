use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Extension;
use axum::extract::State;
use hyper::StatusCode;
use tracing::subscriber::with_default;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;
use underwrap_protocol::service::{AdminStoreBackendKind, AdminStoreCreateRequest};

use super::*;
use crate::admin_api::stores::crud::{handle_admin_store_create, handle_admin_stores_list};
use crate::http::responses::Body;
use crate::store_registry::StoreState;
use crate::test_support::{admin_principal, crud_state, run_handler};

#[derive(Clone, Default)]
struct BufWriter {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl BufWriter {
    fn as_string(&self) -> String {
        String::from_utf8_lossy(&self.inner.lock().unwrap()).into_owned()
    }
}

impl io::Write for BufWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for BufWriter {
    type Writer = BufWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn capture<R>(run: impl FnOnce() -> R) -> (R, String) {
    let writer = BufWriter::default();
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(writer.clone())
        .with_ansi(false)
        .with_target(false);
    let subscriber = Registry::default().with(layer);
    let result = with_default(subscriber, run);
    (result, writer.as_string())
}

fn create_fixture(state: Arc<ListenerState>, principal: &Principal) -> (String, String, PathBuf) {
    let (status, body) = run_handler(handle_admin_store_create(
        State(state.clone()),
        Extension(principal.clone()),
        Body(AdminStoreCreateRequest {
            name: "audit vault".into(),
            backend: AdminStoreBackendKind::Encrypted,
            description: None,
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    let store_id = body["store"]["id"].as_str().unwrap().to_string();
    let old_key_hex = body["key_hex"].as_str().unwrap().to_string();
    let canonical = match &state
        .shared
        .stores
        .read()
        .unwrap()
        .entry(&store_id)
        .unwrap()
        .record
        .storage
    {
        underwrap_protocol::config::StorageConfig::Encrypted { dir } => dir.clone(),
        underwrap_protocol::config::StorageConfig::Plain { .. } => unreachable!(),
    };
    let backend = state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(&store_id);
    backend
        .write("resource/audit-private-path", b"audit-seeded-plaintext")
        .unwrap();
    backend
        .write("control/audit-document", b"audit-decrypted-blob")
        .unwrap();
    (store_id, old_key_hex, canonical)
}

struct AuditHooks {
    failures: HashSet<rotation::RotationPhase>,
    before_publish: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl AuditHooks {
    fn failing(phases: &[rotation::RotationPhase]) -> Self {
        Self {
            failures: phases.iter().copied().collect(),
            before_publish: None,
        }
    }
}

impl rotation::RotationHooks for AuditHooks {
    fn rotation_id(&self) -> String {
        "aaaaaaaaaaaaaaaa".into()
    }

    fn replacement_key(&self) -> [u8; 32] {
        [42; 32]
    }

    fn checkpoint(&self, phase: rotation::RotationPhase) -> io::Result<()> {
        if self.failures.contains(&phase) {
            Err(io::Error::other("secret injected filesystem detail"))
        } else {
            Ok(())
        }
    }

    fn before_publish(&self) {
        if let Some(callback) = &self.before_publish {
            callback();
        }
    }
}

fn assert_forbidden_absent(value: &[u8], forbidden: &[&str], sink: &str) {
    for needle in forbidden {
        assert!(
            !value
                .windows(needle.len())
                .any(|window| window == needle.as_bytes()),
            "{sink} leaked {needle:?}"
        );
    }
}

fn scan_file_contents(root: &Path, forbidden: &[&str]) {
    fn visit(path: &Path, forbidden: &[&str]) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let metadata = std::fs::symlink_metadata(entry.path()).unwrap();
            if metadata.is_dir() {
                visit(&entry.path(), forbidden);
            } else if metadata.is_file() {
                let bytes = std::fs::read(entry.path()).unwrap();
                assert_forbidden_absent(&bytes, forbidden, "persistent file");
            }
        }
    }
    visit(root, forbidden);
}

fn assert_safe_sinks(
    state: &Arc<ListenerState>,
    temp_root: &Path,
    store_id: &str,
    canonical: &Path,
    old_key_hex: &str,
    response: &serde_json::Value,
    logs: &str,
) {
    let new_key_hex = hex::encode([42; 32]);
    let forbidden = [
        old_key_hex,
        new_key_hex.as_str(),
        "audit-seeded-plaintext",
        "audit-decrypted-blob",
        "resource/audit-private-path",
    ];
    assert_forbidden_absent(
        serde_json::to_string(response).unwrap().as_bytes(),
        &forbidden,
        "error response",
    );
    assert_forbidden_absent(logs.as_bytes(), &forbidden, "audit log");
    let (_, later_list) = run_handler(handle_admin_stores_list(State(state.clone())));
    assert_forbidden_absent(
        serde_json::to_string(&later_list).unwrap().as_bytes(),
        &forbidden,
        "later store list",
    );
    if let Some(entry) = state.shared.stores.read().unwrap().entries.get(store_id) {
        assert_forbidden_absent(
            serde_json::to_string(&entry.record).unwrap().as_bytes(),
            &forbidden,
            "registry metadata",
        );
    }
    for entry in std::fs::read_dir(canonical.parent().unwrap()).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        if name.contains("underwrap-rotation") {
            assert_forbidden_absent(name.as_bytes(), &forbidden, "recovery name");
        }
    }
    scan_file_contents(temp_root, &forbidden);
}

#[test]
fn success_and_every_semantic_failpoint_emit_safe_fixed_audits() {
    let cases: &[(&[rotation::RotationPhase], &str)] = &[
        (&[rotation::RotationPhase::ReserveWorkspace], "stage-failed"),
        (&[rotation::RotationPhase::OpenStage], "stage-failed"),
        (&[rotation::RotationPhase::ListSource], "list-failed"),
        (
            &[rotation::RotationPhase::WriteCanary],
            "canary-write-failed",
        ),
        (&[rotation::RotationPhase::ReadSource], "read-failed"),
        (&[rotation::RotationPhase::WriteDestination], "write-failed"),
        (&[rotation::RotationPhase::VerifyStage], "verify-failed"),
        (&[rotation::RotationPhase::DisplaceLive], "swap-failed"),
        (&[rotation::RotationPhase::PromoteStage], "swap-failed"),
        (&[rotation::RotationPhase::OpenPromoted], "reopen-failed"),
        (&[rotation::RotationPhase::VerifyPromoted], "reopen-failed"),
        (
            &[
                rotation::RotationPhase::OpenPromoted,
                rotation::RotationPhase::MovePromotedAside,
            ],
            "rollback-failed",
        ),
        (
            &[
                rotation::RotationPhase::OpenPromoted,
                rotation::RotationPhase::RestoreBackup,
            ],
            "rollback-failed",
        ),
    ];

    for (phases, expected_reason) in cases {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = Arc::new(crud_state(tmp.path()));
        let principal = admin_principal();
        let (store_id, old_key_hex, canonical) = create_fixture(state.clone(), &principal);
        let hooks = AuditHooks::failing(phases);
        let ((status, body), logs) = capture(|| {
            run_handler(rotate_key_with_hooks(
                state.clone(),
                principal.clone(),
                store_id.clone(),
                &hooks,
            ))
        });
        assert!(status.is_server_error(), "{phases:?}: {status} {body}");
        for expected in [
            "action=store.rotate-key",
            &format!("resource=Some(\"store:{store_id}\")"),
            &format!("operation=Some(\"{expected_reason}\")"),
            "result=failure",
        ] {
            assert!(
                logs.contains(expected),
                "{phases:?}: missing {expected:?}: {logs}"
            );
        }
        if *expected_reason == "rollback-failed" {
            assert_eq!(body["code"], "rotation_recovery_required", "{phases:?}");
            assert!(matches!(
                state
                    .shared
                    .stores
                    .read()
                    .unwrap()
                    .entry(&store_id)
                    .unwrap()
                    .state,
                StoreState::Locked
            ));
        } else if phases == &[rotation::RotationPhase::VerifyPromoted] {
            assert_eq!(body["code"], "rotate_reopen_failed");
        }
        assert_safe_sinks(
            &state,
            tmp.path(),
            &store_id,
            &canonical,
            &old_key_hex,
            &body,
            &logs,
        );
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let (store_id, old_key_hex, canonical) = create_fixture(state.clone(), &principal);
    let hooks = AuditHooks::failing(&[]);
    let ((status, body), logs) = capture(|| {
        run_handler(rotate_key_with_hooks(
            state.clone(),
            principal,
            store_id.clone(),
            &hooks,
        ))
    });
    assert_eq!(status, StatusCode::OK);
    assert!(logs.contains("action=store.rotate-key"));
    assert!(logs.contains("operation=None"));
    assert!(logs.contains("result=success"));
    let redacted_body = serde_json::json!({ "warning": body["warning"] });
    assert_safe_sinks(
        &state,
        tmp.path(),
        &store_id,
        &canonical,
        &old_key_hex,
        &redacted_body,
        &logs,
    );
}

#[test]
fn publication_failure_is_audited_and_replacement_key_is_absent_from_sinks() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let (store_id, old_key_hex, canonical) = create_fixture(state.clone(), &principal);
    let callback_state = state.clone();
    let callback_id = store_id.clone();
    let mut hooks = AuditHooks::failing(&[]);
    hooks.before_publish = Some(Arc::new(move || {
        callback_state
            .shared
            .stores
            .write()
            .unwrap()
            .entries
            .remove(&callback_id);
    }));
    let ((status, body), logs) = capture(|| {
        run_handler(rotate_key_with_hooks(
            state.clone(),
            principal,
            store_id.clone(),
            &hooks,
        ))
    });
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "rotation_recovery_required");
    for expected in [
        "action=store.rotate-key",
        &format!("resource=Some(\"store:{store_id}\")"),
        "operation=Some(\"publish-failed\")",
        "result=failure",
    ] {
        assert!(logs.contains(expected), "missing {expected:?}: {logs}");
    }
    assert_safe_sinks(
        &state,
        tmp.path(),
        &store_id,
        &canonical,
        &old_key_hex,
        &body,
        &logs,
    );
}

struct CanonicalDisappearsHooks {
    displaced: PathBuf,
}

impl rotation::RotationHooks for CanonicalDisappearsHooks {
    fn rotation_id(&self) -> String {
        "aaaaaaaaaaaaaaaa".into()
    }

    fn replacement_key(&self) -> [u8; 32] {
        [42; 32]
    }

    fn rename(&self, phase: rotation::RotationPhase, from: &Path, to: &Path) -> io::Result<()> {
        if phase == rotation::RotationPhase::DisplaceLive {
            std::fs::rename(from, &self.displaced)?;
            Err(io::Error::other("canonical unavailable"))
        } else {
            std::fs::rename(from, to)
        }
    }
}

#[test]
fn unusable_canonical_after_first_rename_failure_audits_recovery_and_locks_same_entry() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let principal = admin_principal();
    let (store_id, old_key_hex, canonical) = create_fixture(state.clone(), &principal);
    let hooks = CanonicalDisappearsHooks {
        displaced: tmp.path().join("retained-outside-workspace"),
    };
    let ((status, body), logs) = capture(|| {
        run_handler(rotate_key_with_hooks(
            state.clone(),
            principal,
            store_id.clone(),
            &hooks,
        ))
    });
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "rotation_recovery_required");
    assert!(logs.contains("operation=Some(\"swap-recovery-required\")"));
    assert!(logs.contains("result=failure"));
    assert!(matches!(
        state
            .shared
            .stores
            .read()
            .unwrap()
            .entry(&store_id)
            .unwrap()
            .state,
        StoreState::Locked
    ));
    assert!(!canonical.exists());
    assert_safe_sinks(
        &state,
        tmp.path(),
        &store_id,
        &canonical,
        &old_key_hex,
        &body,
        &logs,
    );
}
