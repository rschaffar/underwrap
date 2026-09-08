use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

struct StreamingHooks {
    copied_started: AtomicUsize,
    copied_released: AtomicUsize,
    compared_started: AtomicUsize,
    compared_released: AtomicUsize,
}

impl StreamingHooks {
    fn new() -> Self {
        Self {
            copied_started: AtomicUsize::new(0),
            copied_released: AtomicUsize::new(0),
            compared_started: AtomicUsize::new(0),
            compared_released: AtomicUsize::new(0),
        }
    }
}

impl MigrationHooks for StreamingHooks {
    fn migration_id(&self) -> String {
        "1234567890abcdef".into()
    }

    fn checkpoint(&self, phase: MigrationPhase) -> io::Result<()> {
        if phase == MigrationPhase::ReadSource {
            let started = self.copied_started.fetch_add(1, Ordering::SeqCst);
            assert_eq!(self.copied_released.load(Ordering::SeqCst), started);
        }
        Ok(())
    }

    fn copied_value_released(&self) {
        self.copied_released.fetch_add(1, Ordering::SeqCst);
    }

    fn before_compare_value_pair(&self) {
        let started = self.compared_started.fetch_add(1, Ordering::SeqCst);
        assert_eq!(self.compared_released.load(Ordering::SeqCst), started);
    }

    fn compared_value_pair_released(&self) {
        self.compared_released.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy)]
enum DisplacedSourceMutation {
    Disappear,
    UnreadableCiphertext,
    ReadableByteMutation,
}

struct MutateDisplacedSourceHooks {
    after_phase: MigrationPhase,
    mutation: DisplacedSourceMutation,
    source_kind: AdminStoreBackendKind,
    source_key: Option<[u8; 32]>,
    fail_open_promoted: bool,
}

impl MigrationHooks for MutateDisplacedSourceHooks {
    fn migration_id(&self) -> String {
        "1234567890abcdef".into()
    }

    fn checkpoint(&self, phase: MigrationPhase) -> io::Result<()> {
        if self.fail_open_promoted && phase == MigrationPhase::OpenPromoted {
            Err(io::Error::other("injected promoted-open failure"))
        } else {
            Ok(())
        }
    }

    fn rename(&self, phase: MigrationPhase, from: &Path, to: &Path) -> io::Result<()> {
        self.checkpoint(phase)?;
        checked_rename(from, to)?;
        if phase == self.after_phase {
            let displaced = match phase {
                MigrationPhase::DisplaceLive | MigrationPhase::RestoreBackup => to.to_path_buf(),
                MigrationPhase::MovePromotedAside => to
                    .parent()
                    .ok_or_else(|| io::Error::other("rollback stage has no parent"))?
                    .join("backup"),
                _ => return Err(io::Error::other("invalid source-mutation phase")),
            };
            match self.mutation {
                DisplacedSourceMutation::Disappear => {
                    let backend = open_generation(&displaced, self.source_kind, self.source_key)?;
                    backend
                        .delete("resources/nested/item")
                        .map_err(|error| io::Error::other(error.to_string()))?;
                }
                DisplacedSourceMutation::UnreadableCiphertext => {
                    std::fs::write(
                        displaced.join("resources/nested/item"),
                        b"invalid ciphertext",
                    )?;
                }
                DisplacedSourceMutation::ReadableByteMutation => {
                    let backend = open_generation(&displaced, self.source_kind, self.source_key)?;
                    backend
                        .write("resources/nested/item", b"mutated source bytes")
                        .map_err(|error| io::Error::other(error.to_string()))?;
                }
            }
        }
        Ok(())
    }
}

#[test]
fn copy_and_verification_release_each_value_before_loading_the_next() {
    let fixture = direction_fixture(AdminStoreBackendKind::Plain);
    fixture
        .source
        .write("resources/another", b"another value")
        .unwrap();
    let hooks = StreamingHooks::new();

    migrate_generation(
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
    .unwrap();

    assert_eq!(hooks.copied_started.load(Ordering::SeqCst), 3);
    assert_eq!(hooks.copied_released.load(Ordering::SeqCst), 3);
    assert_eq!(hooks.compared_started.load(Ordering::SeqCst), 9);
    assert_eq!(hooks.compared_released.load(Ordering::SeqCst), 9);
}

#[test]
fn displaced_source_disappearance_requires_recovery_in_both_directions() {
    for source_kind in [
        AdminStoreBackendKind::Plain,
        AdminStoreBackendKind::Encrypted,
    ] {
        let fixture = direction_fixture(source_kind);
        let hooks = MutateDisplacedSourceHooks {
            after_phase: MigrationPhase::DisplaceLive,
            mutation: DisplacedSourceMutation::Disappear,
            source_kind,
            source_key: (source_kind == AdminStoreBackendKind::Encrypted).then_some([4; 32]),
            fail_open_promoted: false,
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
        .expect_err("a changed displaced source must fail closed");

        assert_eq!(error_code(failure), "migration_recovery_required");
        assert!(!fixture.canonical.exists());
        let workspace = workspaces(fixture.canonical.parent().unwrap())
            .into_iter()
            .next()
            .unwrap();
        assert!(workspace.join("backup").is_dir());
        assert!(workspace.join("stage").is_dir());
        assert_restart_state(&fixture, true);
    }
}

#[test]
fn unreadable_displaced_encrypted_source_requires_recovery() {
    let fixture = direction_fixture(AdminStoreBackendKind::Encrypted);
    let hooks = MutateDisplacedSourceHooks {
        after_phase: MigrationPhase::DisplaceLive,
        mutation: DisplacedSourceMutation::UnreadableCiphertext,
        source_kind: fixture.source_kind,
        source_key: Some([4; 32]),
        fail_open_promoted: false,
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
    .expect_err("an unreadable displaced source must fail closed");

    assert_eq!(error_code(failure), "migration_recovery_required");
    assert!(!fixture.canonical.exists());
    assert_restart_state(&fixture, true);
}

#[test]
fn readable_same_id_source_mutation_requires_recovery_and_preserves_generations() {
    for source_kind in [
        AdminStoreBackendKind::Plain,
        AdminStoreBackendKind::Encrypted,
    ] {
        for after_phase in [
            MigrationPhase::DisplaceLive,
            MigrationPhase::MovePromotedAside,
            MigrationPhase::RestoreBackup,
        ] {
            let fixture = direction_fixture(source_kind);
            let hooks = MutateDisplacedSourceHooks {
                after_phase,
                mutation: DisplacedSourceMutation::ReadableByteMutation,
                source_kind,
                source_key: (source_kind == AdminStoreBackendKind::Encrypted).then_some([4; 32]),
                fail_open_promoted: after_phase != MigrationPhase::DisplaceLive,
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
            .expect_err("a readable changed source generation must fail closed");

            assert_eq!(error_code(failure), "migration_recovery_required");
            let workspace = workspaces(fixture.canonical.parent().unwrap())
                .into_iter()
                .next()
                .unwrap();
            let retained_target = open_generation(
                &workspace.join("stage"),
                fixture.target_kind,
                fixture.target_key_bytes,
            )
            .unwrap();
            let target_value = retained_target
                .read("resources/nested/item")
                .unwrap()
                .unwrap();
            assert!(target_value.as_ref() == b"resource bytes");
            drop(target_value);

            let changed_source_path = if after_phase == MigrationPhase::RestoreBackup {
                fixture.canonical.clone()
            } else {
                workspace.join("backup")
            };
            let changed_source = open_generation(
                &changed_source_path,
                source_kind,
                (source_kind == AdminStoreBackendKind::Encrypted).then_some([4; 32]),
            )
            .unwrap();
            let changed_value = changed_source
                .read("resources/nested/item")
                .unwrap()
                .unwrap();
            assert!(changed_value.as_ref() == b"mutated source bytes");
            drop(changed_value);

            if after_phase == MigrationPhase::RestoreBackup {
                assert!(fixture.canonical.is_dir());
                assert!(!workspace.join("backup").exists());
            } else {
                assert!(!fixture.canonical.exists());
                assert!(workspace.join("backup").is_dir());
            }
            assert_restart_state(&fixture, true);
        }
    }
}

#[test]
fn rollback_revalidates_displaced_source_around_checked_renames() {
    for source_kind in [
        AdminStoreBackendKind::Plain,
        AdminStoreBackendKind::Encrypted,
    ] {
        for after_phase in [
            MigrationPhase::MovePromotedAside,
            MigrationPhase::RestoreBackup,
        ] {
            let fixture = direction_fixture(source_kind);
            let hooks = MutateDisplacedSourceHooks {
                after_phase,
                mutation: DisplacedSourceMutation::Disappear,
                source_kind,
                source_key: (source_kind == AdminStoreBackendKind::Encrypted).then_some([4; 32]),
                fail_open_promoted: true,
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
            .expect_err("rollback must revalidate its source around checked renames");

            assert_eq!(error_code(failure), "migration_recovery_required");
            let workspace = workspaces(fixture.canonical.parent().unwrap())
                .into_iter()
                .next()
                .unwrap();
            assert!(workspace.join("stage").is_dir());
            match after_phase {
                MigrationPhase::MovePromotedAside => {
                    assert!(!fixture.canonical.exists());
                    assert!(workspace.join("backup").is_dir());
                }
                MigrationPhase::RestoreBackup => {
                    assert!(fixture.canonical.is_dir());
                    assert!(!workspace.join("backup").exists());
                }
                _ => unreachable!(),
            }
            assert_restart_state(&fixture, true);
        }
    }
}
