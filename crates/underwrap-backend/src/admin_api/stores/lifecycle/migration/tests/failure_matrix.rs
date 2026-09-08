use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

const NO_RENAMES: &[MigrationPhase] = &[];
const DISPLACE: &[MigrationPhase] = &[MigrationPhase::DisplaceLive];
const DISPLACE_PROMOTE: &[MigrationPhase] =
    &[MigrationPhase::DisplaceLive, MigrationPhase::PromoteStage];
const RESTORE_UNPROMOTED: &[MigrationPhase] = &[
    MigrationPhase::DisplaceLive,
    MigrationPhase::PromoteStage,
    MigrationPhase::RestoreBackup,
];
const RESTORE_PROMOTED: &[MigrationPhase] = &[
    MigrationPhase::DisplaceLive,
    MigrationPhase::PromoteStage,
    MigrationPhase::MovePromotedAside,
    MigrationPhase::RestoreBackup,
];
const FAIL_MOVE_PROMOTED: &[MigrationPhase] = &[
    MigrationPhase::DisplaceLive,
    MigrationPhase::PromoteStage,
    MigrationPhase::MovePromotedAside,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Applicability {
    BothDirections,
    EncryptedTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CallbackResult {
    Success,
    Failure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CanonicalGeneration {
    Source,
    Target,
    Missing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordKind {
    Source,
    Target,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceLayout {
    None,
    SourceWithIncompleteStage,
    TargetWithSourceBackup,
    MissingWithTargetStageAndSourceBackup,
    SourceWithTargetStage,
}

#[derive(Clone, Copy)]
struct FailureCase {
    label: &'static str,
    applicability: Applicability,
    failpoints: &'static [Failpoint],
    persist_result: CallbackResult,
    restore_result: CallbackResult,
    expected_code: &'static str,
    expected_disposition: FailureDisposition,
    target_record_persisted: bool,
    canonical: CanonicalGeneration,
    record_kind: RecordKind,
    workspace: WorkspaceLayout,
    persist_calls: usize,
    restore_calls: usize,
    renames: &'static [MigrationPhase],
}

const BOTH: Applicability = Applicability::BothDirections;
const TARGET_ENCRYPTED: Applicability = Applicability::EncryptedTarget;
const OK: CallbackResult = CallbackResult::Success;
const FAIL: CallbackResult = CallbackResult::Failure;
const RESTORE: FailureDisposition = FailureDisposition::RestoreSource;
const RECOVERY: FailureDisposition = FailureDisposition::RecoveryRequired;
const SOURCE: CanonicalGeneration = CanonicalGeneration::Source;
const TARGET: CanonicalGeneration = CanonicalGeneration::Target;
const MISSING: CanonicalGeneration = CanonicalGeneration::Missing;
const SOURCE_RECORD: RecordKind = RecordKind::Source;
const TARGET_RECORD: RecordKind = RecordKind::Target;
const NO_WORKSPACE: WorkspaceLayout = WorkspaceLayout::None;

#[path = "failure_matrix/cases.rs"]
mod cases;
use cases::CASES;

const fn phase_case(
    label: &'static str,
    applicability: Applicability,
    failpoints: &'static [Failpoint],
    expected_code: &'static str,
    renames: &'static [MigrationPhase],
) -> FailureCase {
    FailureCase {
        label,
        applicability,
        failpoints,
        persist_result: OK,
        restore_result: OK,
        expected_code,
        expected_disposition: RESTORE,
        target_record_persisted: false,
        canonical: SOURCE,
        record_kind: SOURCE_RECORD,
        workspace: NO_WORKSPACE,
        persist_calls: 0,
        restore_calls: 0,
        renames,
    }
}

const fn phase_case_with_rollback(
    label: &'static str,
    failpoints: &'static [Failpoint],
    expected_code: &'static str,
    renames: &'static [MigrationPhase],
) -> FailureCase {
    phase_case(label, BOTH, failpoints, expected_code, renames)
}

struct RecordCallbacks {
    kind: Mutex<RecordKind>,
    persist_calls: AtomicUsize,
    restore_calls: AtomicUsize,
}

impl RecordCallbacks {
    fn new() -> Self {
        Self {
            kind: Mutex::new(SOURCE_RECORD),
            persist_calls: AtomicUsize::new(0),
            restore_calls: AtomicUsize::new(0),
        }
    }
}

fn record_for_kind(
    fixture: &DirectionFixture,
    kind: RecordKind,
) -> underwrap_protocol::config::StoreRecord {
    let backend = match kind {
        RecordKind::Source => fixture.source_kind,
        RecordKind::Target => fixture.target_kind,
    };
    let mut record = fixture_record(fixture);
    record.storage = match backend {
        AdminStoreBackendKind::Plain => underwrap_protocol::config::StorageConfig::Plain {
            dir: fixture.canonical.clone(),
        },
        AdminStoreBackendKind::Encrypted => underwrap_protocol::config::StorageConfig::Encrypted {
            dir: fixture.canonical.clone(),
        },
    };
    record
}

fn key_for_kind(fixture: &DirectionFixture, kind: AdminStoreBackendKind) -> Option<[u8; 32]> {
    match kind {
        AdminStoreBackendKind::Plain => None,
        AdminStoreBackendKind::Encrypted if fixture.source_kind == kind => Some([4; 32]),
        AdminStoreBackendKind::Encrypted => fixture.target_key_bytes,
    }
}

fn assert_generation(
    fixture: &DirectionFixture,
    path: &Path,
    kind: AdminStoreBackendKind,
    label: &str,
) {
    let backend = open_generation(path, kind, key_for_kind(fixture, kind))
        .unwrap_or_else(|_| panic!("{label}: generation did not open as the expected kind"));
    let expected_ids = match kind {
        AdminStoreBackendKind::Plain => vec!["control/policy", "resources/nested/item"],
        AdminStoreBackendKind::Encrypted => {
            vec!["control/canary", "control/policy", "resources/nested/item"]
        }
    };
    let ids = backend
        .list("")
        .unwrap_or_else(|_| panic!("{label}: generation id listing failed"));
    assert!(ids == expected_ids, "{label}: generation id set changed");
    for (id, expected) in [
        ("control/policy", b"control bytes".as_slice()),
        ("resources/nested/item", b"resource bytes".as_slice()),
    ] {
        let value = backend
            .read(id)
            .unwrap_or_else(|_| panic!("{label}: logical value read failed"))
            .unwrap_or_else(|| panic!("{label}: logical value is missing"));
        assert!(
            value.as_ref() == expected,
            "{label}: logical value bytes changed"
        );
    }
    let canary = backend
        .read(underwrap_backend_store::ENCRYPTED_CANARY_ID)
        .unwrap_or_else(|_| panic!("{label}: canary read failed"));
    match kind {
        AdminStoreBackendKind::Plain => {
            assert!(
                canary.is_none(),
                "{label}: plain generation retained a canary"
            );
        }
        AdminStoreBackendKind::Encrypted => {
            let canary = canary.unwrap_or_else(|| panic!("{label}: encrypted canary is missing"));
            assert!(
                canary.as_ref() == underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
                "{label}: encrypted canary changed"
            );
        }
    }
}

fn assert_manifest(fixture: &DirectionFixture, workspace: &Path, label: &str) {
    let manifest = std::fs::read(workspace.join("manifest.json"))
        .unwrap_or_else(|_| panic!("{label}: migration manifest is missing"));
    let manifest: serde_json::Value = serde_json::from_slice(&manifest)
        .unwrap_or_else(|_| panic!("{label}: migration manifest is malformed"));
    assert!(
        manifest["version"] == 1,
        "{label}: manifest version changed"
    );
    assert!(
        manifest["store_id"] == "store_a",
        "{label}: manifest store id changed"
    );
    assert!(
        manifest["source_backend"] == serde_json::to_value(fixture.source_kind).unwrap(),
        "{label}: manifest source kind changed"
    );
    assert!(
        manifest["target_backend"] == serde_json::to_value(fixture.target_kind).unwrap(),
        "{label}: manifest target kind changed"
    );
}

fn assert_workspace(
    fixture: &DirectionFixture,
    before: &[crate::test_support::TreeEntry],
    layout: WorkspaceLayout,
    label: &str,
) {
    let paths = workspaces(fixture.canonical.parent().unwrap());
    if layout == WorkspaceLayout::None {
        assert!(paths.is_empty(), "{label}: migration workspace remained");
        return;
    }
    let expected_workspace = fixture
        .canonical
        .parent()
        .unwrap()
        .join(".store_a.underwrap-migration-1234567890abcdef");
    assert!(
        paths.len() == 1 && paths[0] == expected_workspace,
        "{label}: migration workspace selection changed"
    );
    let workspace = &paths[0];
    let mut names = std::fs::read_dir(workspace)
        .unwrap_or_else(|_| panic!("{label}: migration workspace is unreadable"))
        .map(|entry| {
            entry
                .unwrap_or_else(|_| panic!("{label}: migration workspace entry is unreadable"))
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    let expected_names = match layout {
        WorkspaceLayout::None => unreachable!(),
        WorkspaceLayout::SourceWithIncompleteStage | WorkspaceLayout::SourceWithTargetStage => {
            vec!["manifest.json", "stage"]
        }
        WorkspaceLayout::TargetWithSourceBackup => vec!["backup", "manifest.json"],
        WorkspaceLayout::MissingWithTargetStageAndSourceBackup => {
            vec!["backup", "manifest.json", "stage"]
        }
    };
    assert!(
        names == expected_names,
        "{label}: workspace topology changed"
    );
    assert_manifest(fixture, workspace, label);

    if matches!(
        layout,
        WorkspaceLayout::TargetWithSourceBackup
            | WorkspaceLayout::MissingWithTargetStageAndSourceBackup
    ) {
        let backup = workspace.join("backup");
        assert!(
            tree_snapshot(&backup).as_slice() == before,
            "{label}: retained source generation bytes changed"
        );
        assert_generation(fixture, &backup, fixture.source_kind, label);
    }
    if matches!(
        layout,
        WorkspaceLayout::MissingWithTargetStageAndSourceBackup
            | WorkspaceLayout::SourceWithTargetStage
    ) {
        assert_generation(
            fixture,
            &workspace.join("stage"),
            fixture.target_kind,
            label,
        );
    }
    if layout == WorkspaceLayout::SourceWithIncompleteStage {
        let stage = tree_snapshot(&workspace.join("stage"));
        assert!(stage.len() == 1, "{label}: incomplete stage gained residue");
        assert!(stage[0].kind == "directory", "{label}: stage kind changed");
    }
}

fn assert_startup(
    fixture: &DirectionFixture,
    record_kind: RecordKind,
    recovery_required: bool,
    label: &str,
) {
    let entries = crate::store_registry::open_initial_stores(HashMap::from([(
        "store_a".into(),
        record_for_kind(fixture, record_kind),
    )]))
    .unwrap_or_else(|_| panic!("{label}: startup store opening failed"));
    let state = &entries["store_a"].state;
    if recovery_required {
        assert!(
            matches!(state, crate::store_registry::StoreState::RecoveryRequired),
            "{label}: restart did not require recovery"
        );
        return;
    }
    let expected_kind = match record_kind {
        RecordKind::Source => fixture.source_kind,
        RecordKind::Target => fixture.target_kind,
    };
    match expected_kind {
        AdminStoreBackendKind::Plain => assert!(
            matches!(state, crate::store_registry::StoreState::Ready(_)),
            "{label}: restart did not reopen the plain source"
        ),
        AdminStoreBackendKind::Encrypted => assert!(
            matches!(state, crate::store_registry::StoreState::Locked),
            "{label}: restart did not lock the encrypted source"
        ),
    }
}

#[test]
fn occurrence_sensitive_failure_matrix_covers_both_migration_directions() {
    for source_kind in [
        AdminStoreBackendKind::Plain,
        AdminStoreBackendKind::Encrypted,
    ] {
        for case in CASES {
            if case.applicability == TARGET_ENCRYPTED && source_kind != AdminStoreBackendKind::Plain
            {
                continue;
            }
            let label = format!("{:?} {}", source_kind, case.label);
            let fixture = direction_fixture(source_kind);
            let before = tree_snapshot(&fixture.canonical);
            let hooks = TestHooks::new(case.failpoints);
            let callbacks = RecordCallbacks::new();
            let failure = migrate_generation(
                "store_a",
                &fixture.canonical,
                fixture.source.as_ref(),
                fixture.source_kind,
                fixture.target_kind,
                fixture.target_key.as_ref(),
                &hooks,
                || {
                    callbacks.persist_calls.fetch_add(1, Ordering::SeqCst);
                    if case.persist_result == FAIL {
                        anyhow::bail!("injected record persistence failure");
                    }
                    *callbacks.kind.lock().unwrap() = TARGET_RECORD;
                    Ok(())
                },
                || {
                    callbacks.restore_calls.fetch_add(1, Ordering::SeqCst);
                    if case.restore_result == FAIL {
                        anyhow::bail!("injected record restoration failure");
                    }
                    *callbacks.kind.lock().unwrap() = SOURCE_RECORD;
                    Ok(())
                },
            )
            .unwrap_err();
            let disposition = failure.disposition;
            let target_record_persisted = failure.target_record_persisted;
            let code = error_code(failure);

            assert!(code == case.expected_code, "{label}: public code changed");
            assert!(
                disposition == case.expected_disposition,
                "{label}: failure disposition changed"
            );
            assert!(
                target_record_persisted == case.target_record_persisted,
                "{label}: persisted-record marker changed"
            );
            assert!(
                callbacks.persist_calls.load(Ordering::SeqCst) == case.persist_calls,
                "{label}: target-record callback count changed"
            );
            assert!(
                callbacks.restore_calls.load(Ordering::SeqCst) == case.restore_calls,
                "{label}: source-record callback count changed"
            );
            assert!(
                *callbacks.kind.lock().unwrap() == case.record_kind,
                "{label}: record kind changed"
            );
            assert!(
                hooks.rename_trace().as_slice() == case.renames,
                "{label}: rename ordering changed"
            );
            for failpoint in case.failpoints {
                assert!(
                    hooks.occurrence_count(failpoint.phase) == failpoint.occurrence,
                    "{label}: selected checkpoint occurrence was not reached exactly"
                );
            }

            match case.canonical {
                CanonicalGeneration::Source => {
                    assert!(
                        tree_snapshot(&fixture.canonical).as_slice() == before.as_slice(),
                        "{label}: canonical source generation bytes changed"
                    );
                    assert_generation(&fixture, &fixture.canonical, fixture.source_kind, &label);
                }
                CanonicalGeneration::Target => {
                    assert_generation(&fixture, &fixture.canonical, fixture.target_kind, &label);
                }
                CanonicalGeneration::Missing => assert!(
                    !fixture.canonical.exists(),
                    "{label}: canonical path unexpectedly exists"
                ),
            }
            assert_workspace(&fixture, &before, case.workspace, &label);

            let recovery_required = case.expected_disposition == RECOVERY;
            if !recovery_required {
                assert!(
                    case.canonical == SOURCE
                        && case.record_kind == SOURCE_RECORD
                        && case.workspace == NO_WORKSPACE,
                    "{label}: phase-specific failure lacked exact restoration"
                );
            } else {
                assert!(
                    case.workspace != NO_WORKSPACE,
                    "{label}: recovery-required failure lost its useful workspace"
                );
            }
            assert_startup(&fixture, case.record_kind, recovery_required, &label);
        }
    }
}
