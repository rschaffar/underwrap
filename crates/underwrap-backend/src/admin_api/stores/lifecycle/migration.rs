use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use serde::Serialize;
use underwrap_backend_store::{EncryptedBackend, PlainBackend, StorageBackend};
use underwrap_protocol::service::AdminStoreBackendKind;

use crate::admin_api::AdminError;

use super::workspace::{
    checked_rename, create_private_directory, require_absent, require_directory,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum MigrationPhase {
    ReserveWorkspace,
    WriteManifest,
    OpenStage,
    ListSource,
    ValidateCanary,
    ReadSource,
    WriteDestination,
    VerifyStage,
    DisplaceLive,
    PromoteStage,
    OpenPromoted,
    VerifyPromoted,
    MovePromotedAside,
    RestoreBackup,
    PersistRecord,
    RestoreRecord,
    Cleanup,
}

pub(super) trait MigrationHooks: Sync {
    fn migration_id(&self) -> String;

    fn checkpoint(&self, _phase: MigrationPhase) -> io::Result<()> {
        Ok(())
    }

    fn rename(&self, phase: MigrationPhase, from: &Path, to: &Path) -> io::Result<()> {
        self.checkpoint(phase)?;
        checked_rename(from, to)
    }
}

pub(super) struct ProductionHooks;

impl MigrationHooks for ProductionHooks {
    fn migration_id(&self) -> String {
        crate::ids::random_hex_8()
    }
}

pub(super) struct MigrationKey([u8; 32]);

impl MigrationKey {
    pub(super) fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn copy_bytes(&self) -> [u8; 32] {
        self.0
    }
}

impl Drop for MigrationKey {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

pub(super) struct MigrationSuccess {
    pub(super) backend: Arc<dyn StorageBackend>,
    pub(super) has_secrets: bool,
}

impl std::fmt::Debug for MigrationSuccess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MigrationSuccess")
            .field("backend", &"<storage backend>")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FailureDisposition {
    RestoreSource,
    RecoveryRequired,
}

#[derive(Debug)]
pub(super) struct MigrationFailure {
    pub(super) error: AdminError,
    pub(super) disposition: FailureDisposition,
    pub(super) audit_reason: &'static str,
    pub(super) target_record_persisted: bool,
}

fn failure(code: &'static str, message: &'static str, reason: &'static str) -> MigrationFailure {
    MigrationFailure {
        error: AdminError::internal_with_code(code, message),
        disposition: FailureDisposition::RestoreSource,
        audit_reason: reason,
        target_record_persisted: false,
    }
}

fn recovery(reason: &'static str) -> MigrationFailure {
    MigrationFailure {
        error: AdminError::migration_recovery_required(),
        disposition: FailureDisposition::RecoveryRequired,
        audit_reason: reason,
        target_record_persisted: false,
    }
}

fn recovery_after_persist(reason: &'static str) -> MigrationFailure {
    MigrationFailure {
        error: AdminError::migration_recovery_required(),
        disposition: FailureDisposition::RecoveryRequired,
        audit_reason: reason,
        target_record_persisted: true,
    }
}

#[derive(Serialize)]
struct MigrationManifest<'a> {
    version: u8,
    store_id: &'a str,
    source_backend: AdminStoreBackendKind,
    target_backend: AdminStoreBackendKind,
}

fn reserve_workspace(
    parent: &Path,
    store_id: &str,
    source: AdminStoreBackendKind,
    target: AdminStoreBackendKind,
    hooks: &dyn MigrationHooks,
) -> Result<PathBuf, MigrationFailure> {
    for _ in 0..32 {
        hooks
            .checkpoint(MigrationPhase::ReserveWorkspace)
            .map_err(|_| {
                failure(
                    "migrate_stage_failed",
                    "could not reserve the migration workspace",
                    "stage-failed",
                )
            })?;
        let id = hooks.migration_id();
        if id.len() != 16
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(failure(
                "migrate_stage_failed",
                "could not reserve the migration workspace",
                "stage-failed",
            ));
        }
        let workspace = parent.join(format!(".{store_id}.underwrap-migration-{id}"));
        match create_private_directory(&workspace) {
            Ok(()) => {
                if hooks.checkpoint(MigrationPhase::WriteManifest).is_err()
                    || write_manifest(&workspace, store_id, source, target).is_err()
                {
                    return match cleanup_workspace(&workspace, hooks) {
                        Ok(()) => Err(failure(
                            "migrate_stage_failed",
                            "could not reserve the migration workspace",
                            "stage-failed",
                        )),
                        Err(()) => Err(recovery("cleanup-failed")),
                    };
                }
                return Ok(workspace);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                return Err(failure(
                    "migrate_stage_failed",
                    "could not reserve the migration workspace",
                    "stage-failed",
                ));
            }
        }
    }
    Err(failure(
        "migrate_stage_failed",
        "could not reserve the migration workspace",
        "stage-failed",
    ))
}

fn write_manifest(
    workspace: &Path,
    store_id: &str,
    source: AdminStoreBackendKind,
    target: AdminStoreBackendKind,
) -> io::Result<()> {
    let path = workspace.join("manifest.json");
    require_absent(&path)?;
    let bytes = serde_json::to_vec_pretty(&MigrationManifest {
        version: 1,
        store_id,
        source_backend: source,
        target_backend: target,
    })
    .map_err(io::Error::other)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()
}

fn validate_workspace_for_cleanup(workspace: &Path) -> io::Result<()> {
    require_directory(workspace, true)?;
    match std::fs::symlink_metadata(workspace.join("manifest.json")) {
        Ok(manifest) if manifest.file_type().is_file() && !manifest.file_type().is_symlink() => {}
        Ok(_) => return Err(io::Error::other("migration manifest is not a regular file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    for name in ["stage", "backup"] {
        let path = workspace.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => require_directory(&path, true)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn cleanup_workspace(workspace: &Path, hooks: &dyn MigrationHooks) -> Result<(), ()> {
    if hooks.checkpoint(MigrationPhase::Cleanup).is_err()
        || validate_workspace_for_cleanup(workspace).is_err()
        || std::fs::remove_dir_all(workspace).is_err()
    {
        return Err(());
    }
    require_absent(workspace).map_err(|_| ())
}

fn cleanup_before_displacement(
    workspace: &Path,
    hooks: &dyn MigrationHooks,
    phase_failure: MigrationFailure,
) -> MigrationFailure {
    match cleanup_workspace(workspace, hooks) {
        Ok(()) => phase_failure,
        Err(()) => recovery("cleanup-failed"),
    }
}

fn stage_failure(
    workspace: &Path,
    hooks: &dyn MigrationHooks,
    code: &'static str,
    message: &'static str,
    reason: &'static str,
) -> MigrationFailure {
    cleanup_before_displacement(workspace, hooks, failure(code, message, reason))
}

fn open_target(
    path: &Path,
    kind: AdminStoreBackendKind,
    key: Option<&MigrationKey>,
) -> anyhow::Result<Arc<dyn StorageBackend>> {
    match kind {
        AdminStoreBackendKind::Plain => Ok(Arc::new(PlainBackend::create(path)?)),
        AdminStoreBackendKind::Encrypted => {
            let key = key.context("encrypted migration target lacks key")?;
            let mut bytes = key.copy_bytes();
            let result = EncryptedBackend::create_with_key(path, bytes);
            bytes.fill(0);
            Ok(Arc::new(result?))
        }
    }
}

fn reopen_target(
    path: &Path,
    kind: AdminStoreBackendKind,
    key: Option<&MigrationKey>,
) -> anyhow::Result<Arc<dyn StorageBackend>> {
    match kind {
        AdminStoreBackendKind::Plain => Ok(Arc::new(PlainBackend::open_existing(path)?)),
        AdminStoreBackendKind::Encrypted => {
            let key = key.context("encrypted migration target lacks key")?;
            let mut bytes = key.copy_bytes();
            let result = EncryptedBackend::open_existing_with_key(path, bytes);
            bytes.fill(0);
            Ok(Arc::new(result?))
        }
    }
}

fn collect_source(
    source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    workspace: &Path,
    hooks: &dyn MigrationHooks,
) -> Result<BTreeMap<String, underwrap_protocol::message::SecretBytes>, MigrationFailure> {
    if hooks.checkpoint(MigrationPhase::ListSource).is_err() {
        return Err(stage_failure(
            workspace,
            hooks,
            "migrate_list_failed",
            "could not list the migration source",
            "list-failed",
        ));
    }
    let ids = source.list("").map_err(|_| {
        stage_failure(
            workspace,
            hooks,
            "migrate_list_failed",
            "could not list the migration source",
            "list-failed",
        )
    })?;
    let contains_canary = ids
        .iter()
        .any(|id| id == underwrap_backend_store::ENCRYPTED_CANARY_ID);
    if hooks.checkpoint(MigrationPhase::ValidateCanary).is_err()
        || (source_kind == AdminStoreBackendKind::Plain && contains_canary)
        || (source_kind == AdminStoreBackendKind::Encrypted
            && !matches!(
                source.read(underwrap_backend_store::ENCRYPTED_CANARY_ID),
                Ok(Some(ref value))
                    if value.as_ref() == underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT
            ))
    {
        return Err(stage_failure(
            workspace,
            hooks,
            "migrate_canary_failed",
            "the migration source canary did not verify",
            "canary-failed",
        ));
    }

    let mut values = BTreeMap::new();
    for id in ids {
        if id == underwrap_backend_store::ENCRYPTED_CANARY_ID {
            continue;
        }
        if hooks.checkpoint(MigrationPhase::ReadSource).is_err() {
            return Err(stage_failure(
                workspace,
                hooks,
                "migrate_read_failed",
                "could not read a migration source value",
                "read-failed",
            ));
        }
        match source.read(&id) {
            Ok(Some(value)) => {
                values.insert(id, value);
            }
            Ok(None) | Err(_) => {
                return Err(stage_failure(
                    workspace,
                    hooks,
                    "migrate_read_failed",
                    "could not read a migration source value",
                    "read-failed",
                ));
            }
        }
    }
    Ok(values)
}

fn verify_backend(
    backend: &dyn StorageBackend,
    kind: AdminStoreBackendKind,
    values: &BTreeMap<String, underwrap_protocol::message::SecretBytes>,
) -> bool {
    let mut expected = values.keys().cloned().collect::<BTreeSet<_>>();
    if kind == AdminStoreBackendKind::Encrypted {
        expected.insert(underwrap_backend_store::ENCRYPTED_CANARY_ID.to_string());
    }
    let Ok(actual) = backend.list("") else {
        return false;
    };
    if actual.into_iter().collect::<BTreeSet<_>>() != expected {
        return false;
    }
    if kind == AdminStoreBackendKind::Encrypted
        && !matches!(
            backend.read(underwrap_backend_store::ENCRYPTED_CANARY_ID),
            Ok(Some(ref value))
                if value.as_ref() == underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT
        )
    {
        return false;
    }
    values.iter().all(|(id, expected)| {
        matches!(backend.read(id), Ok(Some(ref actual)) if actual.as_ref() == expected.as_ref())
    })
}

fn source_is_restored(
    canonical: &Path,
    source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    values: &BTreeMap<String, underwrap_protocol::message::SecretBytes>,
) -> bool {
    require_directory(canonical, false).is_ok() && verify_backend(source, source_kind, values)
}

struct RollbackContext<'a> {
    canonical: &'a Path,
    stage: &'a Path,
    backup: &'a Path,
    workspace: &'a Path,
    hooks: &'a dyn MigrationHooks,
}

fn restore_source_generation(
    context: &RollbackContext<'_>,
    source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    values: &BTreeMap<String, underwrap_protocol::message::SecretBytes>,
) -> Result<(), MigrationFailure> {
    context
        .hooks
        .rename(
            MigrationPhase::MovePromotedAside,
            context.canonical,
            context.stage,
        )
        .map_err(|_| recovery("rollback-failed"))?;
    context
        .hooks
        .rename(
            MigrationPhase::RestoreBackup,
            context.backup,
            context.canonical,
        )
        .map_err(|_| recovery("rollback-failed"))?;
    if !source_is_restored(context.canonical, source, source_kind, values) {
        return Err(recovery("rollback-verify-failed"));
    }
    Ok(())
}

fn rollback_promoted(
    context: &RollbackContext<'_>,
    source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    values: &BTreeMap<String, underwrap_protocol::message::SecretBytes>,
) -> Result<(), MigrationFailure> {
    restore_source_generation(context, source, source_kind, values)?;
    cleanup_workspace(context.workspace, context.hooks).map_err(|_| recovery("cleanup-failed"))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn migrate_generation(
    store_id: &str,
    canonical: &Path,
    source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    target_kind: AdminStoreBackendKind,
    target_key: Option<&MigrationKey>,
    hooks: &dyn MigrationHooks,
    mut persist_target: impl FnMut() -> anyhow::Result<()>,
    mut restore_source_record: impl FnMut() -> anyhow::Result<()>,
) -> Result<MigrationSuccess, MigrationFailure> {
    let parent = canonical.parent().ok_or_else(|| {
        failure(
            "migrate_stage_failed",
            "the migration source has no parent",
            "stage-failed",
        )
    })?;
    let workspace = reserve_workspace(parent, store_id, source_kind, target_kind, hooks)?;
    require_directory(canonical, false)
        .map_err(|_| recovery("source-topology-recovery-required"))?;
    let stage = workspace.join("stage");
    let backup = workspace.join("backup");
    require_absent(&stage).map_err(|_| {
        stage_failure(
            &workspace,
            hooks,
            "migrate_stage_failed",
            "the migration stage path is occupied",
            "stage-failed",
        )
    })?;
    require_absent(&backup).map_err(|_| {
        stage_failure(
            &workspace,
            hooks,
            "migrate_stage_failed",
            "the migration backup path is occupied",
            "stage-failed",
        )
    })?;

    if hooks.checkpoint(MigrationPhase::OpenStage).is_err() {
        return Err(stage_failure(
            &workspace,
            hooks,
            "migrate_stage_failed",
            "could not open the migration target",
            "stage-failed",
        ));
    }
    let staged = open_target(&stage, target_kind, target_key).map_err(|_| {
        stage_failure(
            &workspace,
            hooks,
            "migrate_stage_failed",
            "could not open the migration target",
            "stage-failed",
        )
    })?;
    if require_directory(&stage, true).is_err() {
        return Err(stage_failure(
            &workspace,
            hooks,
            "migrate_stage_failed",
            "the migration target topology did not verify",
            "stage-failed",
        ));
    }

    let values = collect_source(source, source_kind, &workspace, hooks)?;
    if target_kind == AdminStoreBackendKind::Encrypted
        && (hooks.checkpoint(MigrationPhase::ValidateCanary).is_err()
            || staged
                .write(
                    underwrap_backend_store::ENCRYPTED_CANARY_ID,
                    underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
                )
                .is_err()
            || !matches!(
                staged.read(underwrap_backend_store::ENCRYPTED_CANARY_ID),
                Ok(Some(ref value))
                    if value.as_ref() == underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT
            ))
    {
        return Err(stage_failure(
            &workspace,
            hooks,
            "migrate_canary_failed",
            "the migration target canary did not verify",
            "canary-failed",
        ));
    }

    for (id, value) in &values {
        if hooks.checkpoint(MigrationPhase::WriteDestination).is_err()
            || staged.write(id, value.as_ref()).is_err()
        {
            return Err(stage_failure(
                &workspace,
                hooks,
                "migrate_write_failed",
                "could not write a migration target value",
                "write-failed",
            ));
        }
    }
    if hooks.checkpoint(MigrationPhase::VerifyStage).is_err()
        || !verify_backend(staged.as_ref(), target_kind, &values)
    {
        return Err(stage_failure(
            &workspace,
            hooks,
            "migrate_verify_failed",
            "the staged migration target did not verify",
            "verify-failed",
        ));
    }

    if hooks
        .rename(MigrationPhase::DisplaceLive, canonical, &backup)
        .is_err()
    {
        if source_is_restored(canonical, source, source_kind, &values) {
            return Err(stage_failure(
                &workspace,
                hooks,
                "migrate_swap_failed",
                "could not displace the migration source",
                "swap-failed",
            ));
        }
        return Err(recovery("swap-recovery-required"));
    }
    if hooks
        .rename(MigrationPhase::PromoteStage, &stage, canonical)
        .is_err()
    {
        if hooks
            .rename(MigrationPhase::RestoreBackup, &backup, canonical)
            .is_err()
            || !source_is_restored(canonical, source, source_kind, &values)
        {
            return Err(recovery("rollback-failed"));
        }
        return match cleanup_workspace(&workspace, hooks) {
            Ok(()) => Err(failure(
                "migrate_swap_failed",
                "could not promote the migration target; the source was restored",
                "swap-failed",
            )),
            Err(()) => Err(recovery("cleanup-failed")),
        };
    }

    drop(staged);
    let rollback = RollbackContext {
        canonical,
        stage: &stage,
        backup: &backup,
        workspace: &workspace,
        hooks,
    };
    if hooks.checkpoint(MigrationPhase::OpenPromoted).is_err() {
        rollback_promoted(&rollback, source, source_kind, &values)?;
        return Err(failure(
            "migrate_reopen_failed",
            "could not reopen the migration target; the source was restored",
            "reopen-failed",
        ));
    }
    let promoted = match reopen_target(canonical, target_kind, target_key) {
        Ok(backend) => backend,
        Err(_) => {
            rollback_promoted(&rollback, source, source_kind, &values)?;
            return Err(failure(
                "migrate_reopen_failed",
                "could not reopen the migration target; the source was restored",
                "reopen-failed",
            ));
        }
    };
    if hooks.checkpoint(MigrationPhase::VerifyPromoted).is_err()
        || !verify_backend(promoted.as_ref(), target_kind, &values)
    {
        drop(promoted);
        rollback_promoted(&rollback, source, source_kind, &values)?;
        return Err(failure(
            "migrate_verify_failed",
            "the promoted migration target did not verify; the source was restored",
            "verify-failed",
        ));
    }

    if hooks.checkpoint(MigrationPhase::PersistRecord).is_err() || persist_target().is_err() {
        drop(promoted);
        restore_source_generation(&rollback, source, source_kind, &values)?;
        if hooks.checkpoint(MigrationPhase::RestoreRecord).is_err()
            || restore_source_record().is_err()
        {
            return Err(recovery("registry-restore-failed"));
        }
        cleanup_workspace(&workspace, hooks).map_err(|_| recovery("cleanup-failed"))?;
        return Err(failure(
            "write_store_registry_failed",
            "could not persist the migrated store record; the source was restored",
            "registry-write-failed",
        ));
    }

    if cleanup_workspace(&workspace, hooks).is_err() {
        return Err(recovery_after_persist("cleanup-failed"));
    }
    let has_secrets = values
        .keys()
        .any(|id| !underwrap_protocol::control::is_control_key(id));
    Ok(MigrationSuccess {
        backend: promoted,
        has_secrets,
    })
}

#[cfg(test)]
mod tests;
