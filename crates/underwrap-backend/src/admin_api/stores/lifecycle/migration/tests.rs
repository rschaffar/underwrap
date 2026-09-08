use std::collections::HashMap;
use std::sync::Mutex;

use axum::response::IntoResponse;

use super::*;
use crate::test_support::{response_body_json, tree_snapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Failpoint {
    phase: MigrationPhase,
    occurrence: usize,
}

impl Failpoint {
    const fn new(phase: MigrationPhase, occurrence: usize) -> Self {
        Self { phase, occurrence }
    }
}

struct TestHooks {
    id: &'static str,
    failpoints: Vec<Failpoint>,
    occurrences: Mutex<HashMap<MigrationPhase, usize>>,
    renames: Mutex<Vec<MigrationPhase>>,
}

impl TestHooks {
    fn new(failpoints: &[Failpoint]) -> Self {
        Self::with_id("1234567890abcdef", failpoints)
    }

    fn with_id(id: &'static str, failpoints: &[Failpoint]) -> Self {
        Self {
            id,
            failpoints: failpoints.to_vec(),
            occurrences: Mutex::new(HashMap::new()),
            renames: Mutex::new(Vec::new()),
        }
    }

    fn occurrence_count(&self, phase: MigrationPhase) -> usize {
        self.occurrences
            .lock()
            .unwrap()
            .get(&phase)
            .copied()
            .unwrap_or(0)
    }

    fn rename_trace(&self) -> Vec<MigrationPhase> {
        self.renames.lock().unwrap().clone()
    }
}

impl MigrationHooks for TestHooks {
    fn migration_id(&self) -> String {
        self.id.into()
    }

    fn checkpoint(&self, phase: MigrationPhase) -> io::Result<()> {
        let occurrence = {
            let mut occurrences = self.occurrences.lock().unwrap();
            let occurrence = occurrences.entry(phase).or_insert(0);
            *occurrence += 1;
            *occurrence
        };
        if self.failpoints.contains(&Failpoint::new(phase, occurrence)) {
            Err(io::Error::other("injected migration failure"))
        } else {
            Ok(())
        }
    }

    fn rename(&self, phase: MigrationPhase, from: &Path, to: &Path) -> io::Result<()> {
        self.renames.lock().unwrap().push(phase);
        self.checkpoint(phase)?;
        checked_rename(from, to)
    }
}

#[derive(Clone, Copy, Debug)]
enum VerificationCorruption {
    Canary,
    MissingId,
    ExtraId,
    ByteMismatch,
}

struct CorruptingHooks {
    phase: MigrationPhase,
    corruption: VerificationCorruption,
    target_kind: AdminStoreBackendKind,
    target_key: Option<[u8; 32]>,
    canonical: PathBuf,
    workspace: PathBuf,
    failpoints: TestHooks,
}

impl MigrationHooks for CorruptingHooks {
    fn migration_id(&self) -> String {
        "1234567890abcdef".into()
    }

    fn checkpoint(&self, phase: MigrationPhase) -> io::Result<()> {
        self.failpoints.checkpoint(phase)?;
        if phase != self.phase {
            return Ok(());
        }
        let path = match phase {
            MigrationPhase::VerifyStage => self.workspace.join("stage"),
            MigrationPhase::VerifyPromoted => self.canonical.clone(),
            _ => return Err(io::Error::other("invalid corruption checkpoint")),
        };
        corrupt_generation(&path, self.target_kind, self.target_key, self.corruption)
    }
}

fn open_generation(
    path: &Path,
    kind: AdminStoreBackendKind,
    key: Option<[u8; 32]>,
) -> io::Result<Box<dyn StorageBackend>> {
    match kind {
        AdminStoreBackendKind::Plain => PlainBackend::open_existing(path)
            .map(|backend| Box::new(backend) as Box<dyn StorageBackend>)
            .map_err(|error| io::Error::other(error.to_string())),
        AdminStoreBackendKind::Encrypted => EncryptedBackend::open_existing_with_key(
            path,
            key.ok_or_else(|| io::Error::other("missing test migration key"))?,
        )
        .map(|backend| Box::new(backend) as Box<dyn StorageBackend>)
        .map_err(|error| io::Error::other(error.to_string())),
    }
}

fn corrupt_generation(
    path: &Path,
    kind: AdminStoreBackendKind,
    key: Option<[u8; 32]>,
    corruption: VerificationCorruption,
) -> io::Result<()> {
    let backend = open_generation(path, kind, key)?;
    match corruption {
        VerificationCorruption::Canary => backend
            .write(underwrap_backend_store::ENCRYPTED_CANARY_ID, b"invalid")
            .map_err(|error| io::Error::other(error.to_string())),
        VerificationCorruption::MissingId => backend
            .delete("resources/nested/item")
            .map(|_| ())
            .map_err(|error| io::Error::other(error.to_string())),
        VerificationCorruption::ExtraId => backend
            .write("resources/extra", b"extra")
            .map_err(|error| io::Error::other(error.to_string())),
        VerificationCorruption::ByteMismatch => backend
            .write("resources/nested/item", b"changed")
            .map_err(|error| io::Error::other(error.to_string())),
    }
}

struct DirectionFixture {
    _tmp: tempfile::TempDir,
    canonical: PathBuf,
    source: Arc<dyn StorageBackend>,
    source_kind: AdminStoreBackendKind,
    target_kind: AdminStoreBackendKind,
    target_key: Option<MigrationKey>,
    target_key_bytes: Option<[u8; 32]>,
}

fn direction_fixture(source_kind: AdminStoreBackendKind) -> DirectionFixture {
    let tmp = tempfile::TempDir::new().unwrap();
    let canonical = tmp.path().join("store_data");
    let source: Arc<dyn StorageBackend> = match source_kind {
        AdminStoreBackendKind::Plain => Arc::new(PlainBackend::create(&canonical).unwrap()),
        AdminStoreBackendKind::Encrypted => {
            let backend = EncryptedBackend::create_with_key(&canonical, [4; 32]).unwrap();
            backend
                .write(
                    underwrap_backend_store::ENCRYPTED_CANARY_ID,
                    underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
                )
                .unwrap();
            Arc::new(backend)
        }
    };
    source
        .write("resources/nested/item", b"resource bytes")
        .unwrap();
    source.write("control/policy", b"control bytes").unwrap();
    let target_kind = match source_kind {
        AdminStoreBackendKind::Plain => AdminStoreBackendKind::Encrypted,
        AdminStoreBackendKind::Encrypted => AdminStoreBackendKind::Plain,
    };
    let target_key_bytes = (target_kind == AdminStoreBackendKind::Encrypted).then_some([7; 32]);
    DirectionFixture {
        _tmp: tmp,
        canonical,
        source,
        source_kind,
        target_kind,
        target_key: target_key_bytes.map(MigrationKey::new),
        target_key_bytes,
    }
}

fn fixture_record(fixture: &DirectionFixture) -> underwrap_protocol::config::StoreRecord {
    underwrap_protocol::config::StoreRecord {
        id: "store_a".into(),
        name: "A".into(),
        description: None,
        created_at: None,
        storage: match fixture.source_kind {
            AdminStoreBackendKind::Plain => underwrap_protocol::config::StorageConfig::Plain {
                dir: fixture.canonical.clone(),
            },
            AdminStoreBackendKind::Encrypted => {
                underwrap_protocol::config::StorageConfig::Encrypted {
                    dir: fixture.canonical.clone(),
                }
            }
        },
    }
}

fn assert_restart_state(fixture: &DirectionFixture, recovery_required: bool) {
    let entries = crate::store_registry::open_initial_stores(HashMap::from([(
        "store_a".into(),
        fixture_record(fixture),
    )]))
    .unwrap();
    let state = &entries["store_a"].state;
    if recovery_required {
        assert!(matches!(
            state,
            crate::store_registry::StoreState::RecoveryRequired
        ));
    } else {
        match fixture.source_kind {
            AdminStoreBackendKind::Plain => {
                assert!(matches!(state, crate::store_registry::StoreState::Ready(_)));
            }
            AdminStoreBackendKind::Encrypted => {
                assert!(matches!(state, crate::store_registry::StoreState::Locked));
            }
        }
    }
}

fn plain_fixture() -> (tempfile::TempDir, PathBuf, PlainBackend) {
    let tmp = tempfile::TempDir::new().unwrap();
    let canonical = tmp.path().join("store_data");
    let backend = PlainBackend::create(&canonical).unwrap();
    backend
        .write("resources/nested/item", b"resource bytes")
        .unwrap();
    backend.write("control/policy", b"control bytes").unwrap();
    (tmp, canonical, backend)
}

fn workspaces(parent: &Path) -> Vec<PathBuf> {
    let mut paths = std::fs::read_dir(parent)
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("underwrap-migration")
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn error_code(failure: MigrationFailure) -> String {
    response_body_json(failure.error.into_response())["code"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn both_directions_preserve_ids_and_logical_bytes() {
    let (tmp, canonical, plain) = plain_fixture();
    let encrypted_key = MigrationKey::new([7; 32]);
    let target_persisted = Mutex::new(false);
    let source_restored = Mutex::new(false);
    let encrypted = migrate_generation(
        "store_a",
        &canonical,
        &plain,
        AdminStoreBackendKind::Plain,
        AdminStoreBackendKind::Encrypted,
        Some(&encrypted_key),
        &TestHooks::new(&[]),
        || {
            *target_persisted.lock().unwrap() = true;
            Ok(())
        },
        || {
            *source_restored.lock().unwrap() = true;
            Ok(())
        },
    )
    .unwrap();
    assert!(*target_persisted.lock().unwrap());
    assert!(!*source_restored.lock().unwrap());
    assert!(encrypted.has_secrets);
    assert!(workspaces(tmp.path()).is_empty());
    assert!(
        encrypted
            .backend
            .read("resources/nested/item")
            .unwrap()
            .unwrap()
            .as_ref()
            == b"resource bytes",
        "encrypted target resource bytes changed"
    );
    assert!(
        encrypted
            .backend
            .read("control/policy")
            .unwrap()
            .unwrap()
            .as_ref()
            == b"control bytes",
        "encrypted target control bytes changed"
    );
    assert!(
        encrypted
            .backend
            .read(underwrap_backend_store::ENCRYPTED_CANARY_ID)
            .unwrap()
            .is_some()
    );

    let plain_again = migrate_generation(
        "store_a",
        &canonical,
        encrypted.backend.as_ref(),
        AdminStoreBackendKind::Encrypted,
        AdminStoreBackendKind::Plain,
        None,
        &TestHooks::with_id("fedcba0987654321", &[]),
        || Ok(()),
        || Ok(()),
    )
    .unwrap();
    assert!(plain_again.has_secrets);
    assert!(workspaces(tmp.path()).is_empty());
    assert_eq!(
        plain_again.backend.list("").unwrap(),
        vec!["control/policy", "resources/nested/item"]
    );
    assert!(
        plain_again
            .backend
            .read("resources/nested/item")
            .unwrap()
            .unwrap()
            .as_ref()
            == b"resource bytes",
        "round-trip resource bytes changed"
    );
}

#[test]
fn staged_and_promoted_verification_detect_real_corruption_in_both_directions() {
    for source_kind in [
        AdminStoreBackendKind::Plain,
        AdminStoreBackendKind::Encrypted,
    ] {
        for phase in [MigrationPhase::VerifyStage, MigrationPhase::VerifyPromoted] {
            for corruption in [
                VerificationCorruption::Canary,
                VerificationCorruption::MissingId,
                VerificationCorruption::ExtraId,
                VerificationCorruption::ByteMismatch,
            ] {
                let fixture = direction_fixture(source_kind);
                let before = tree_snapshot(&fixture.canonical);
                let workspace = fixture
                    .canonical
                    .parent()
                    .unwrap()
                    .join(".store_a.underwrap-migration-1234567890abcdef");
                let hooks = CorruptingHooks {
                    phase,
                    corruption,
                    target_kind: fixture.target_kind,
                    target_key: fixture.target_key_bytes,
                    canonical: fixture.canonical.clone(),
                    workspace,
                    failpoints: TestHooks::new(&[]),
                };
                let failure = migrate_generation(
                    "store_a",
                    &fixture.canonical,
                    fixture.source.as_ref(),
                    fixture.source_kind,
                    fixture.target_kind,
                    fixture.target_key.as_ref(),
                    &hooks,
                    || Ok(()),
                    || Ok(()),
                )
                .expect_err("corrupted target verification must fail");
                assert_eq!(
                    error_code(failure),
                    "migrate_verify_failed",
                    "{source_kind:?} {phase:?} {corruption:?}"
                );
                assert!(
                    tree_snapshot(&fixture.canonical) == before,
                    "{source_kind:?} {phase:?} {corruption:?}: source generation bytes changed"
                );
                assert!(
                    workspaces(fixture.canonical.parent().unwrap()).is_empty(),
                    "{source_kind:?} {phase:?} {corruption:?}"
                );
                assert_restart_state(&fixture, false);
            }
        }
    }
}

#[test]
fn promoted_corruption_with_failed_rollback_is_recovery_required_after_restart() {
    for source_kind in [
        AdminStoreBackendKind::Plain,
        AdminStoreBackendKind::Encrypted,
    ] {
        let fixture = direction_fixture(source_kind);
        let workspace = fixture
            .canonical
            .parent()
            .unwrap()
            .join(".store_a.underwrap-migration-1234567890abcdef");
        let hooks = CorruptingHooks {
            phase: MigrationPhase::VerifyPromoted,
            corruption: VerificationCorruption::ByteMismatch,
            target_kind: fixture.target_kind,
            target_key: fixture.target_key_bytes,
            canonical: fixture.canonical.clone(),
            workspace,
            failpoints: TestHooks::new(&[Failpoint::new(MigrationPhase::MovePromotedAside, 1)]),
        };
        let failure = migrate_generation(
            "store_a",
            &fixture.canonical,
            fixture.source.as_ref(),
            fixture.source_kind,
            fixture.target_kind,
            fixture.target_key.as_ref(),
            &hooks,
            || Ok(()),
            || Ok(()),
        )
        .expect_err("failed rollback after real corruption must fail closed");
        assert_eq!(error_code(failure), "migration_recovery_required");
        assert_eq!(workspaces(fixture.canonical.parent().unwrap()).len(), 1);
        assert_restart_state(&fixture, true);
    }
}

#[cfg(unix)]
#[test]
fn permissive_umask_target_and_workspace_modes_are_exact() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    const CHILD: &str = "UNDERWRAP_MIGRATION_UMASK_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = Command::new(std::env::current_exe().unwrap())
            .arg("admin_api::stores::lifecycle::migration::tests::permissive_umask_target_and_workspace_modes_are_exact")
            .arg("--exact")
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }

    unsafe { libc::umask(0) };
    let (tmp, canonical, plain) = plain_fixture();
    let key = MigrationKey::new([10; 32]);
    let hooks = TestHooks::new(&[Failpoint::new(MigrationPhase::ListSource, 1)]);
    let failure = migrate_generation(
        "store_a",
        &canonical,
        &plain,
        AdminStoreBackendKind::Plain,
        AdminStoreBackendKind::Encrypted,
        Some(&key),
        &hooks,
        || Ok(()),
        || Ok(()),
    )
    .unwrap_err();
    assert_eq!(error_code(failure), "migrate_list_failed");
    assert!(workspaces(tmp.path()).is_empty());
    assert_eq!(
        std::fs::symlink_metadata(&canonical)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o0700
    );
}

#[path = "tests/failure_matrix.rs"]
mod failure_matrix;
#[path = "tests/hardening.rs"]
mod hardening;
