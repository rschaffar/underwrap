use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use serde::Serialize;
use sha2::{Digest, Sha256};
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

    #[cfg(test)]
    fn before_lifecycle_lock(&self) {}

    #[cfg(test)]
    fn copied_value_released(&self) {}

    #[cfg(test)]
    fn before_compare_value_pair(&self) {}

    #[cfg(test)]
    fn compared_value_pair_released(&self) {}

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

#[allow(clippy::too_many_arguments)]
fn stage_failure_with_verified_source(
    source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    inventory: &SourceInventory,
    workspace: &Path,
    hooks: &dyn MigrationHooks,
    code: &'static str,
    message: &'static str,
    reason: &'static str,
) -> MigrationFailure {
    if !source_generation_matches(source, source_kind, inventory) {
        return recovery("source-fingerprint-verify-recovery-required");
    }
    stage_failure(workspace, hooks, code, message, reason)
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

const SOURCE_GENERATION_FINGERPRINT_DOMAIN: &[u8] =
    b"underwrap-store-migration-source-generation-v1\0";

#[derive(Eq, PartialEq)]
struct SourceGenerationFingerprint([u8; 32]);

struct SourceGenerationFingerprintError;

struct SourceInventory {
    ids: BTreeSet<String>,
    has_secrets: bool,
    fingerprint: SourceGenerationFingerprint,
}

impl SourceInventory {
    fn expected_ids(&self, kind: AdminStoreBackendKind) -> BTreeSet<String> {
        let mut expected = self.ids.clone();
        if kind == AdminStoreBackendKind::Encrypted {
            expected.insert(underwrap_backend_store::ENCRYPTED_CANARY_ID.to_string());
        }
        expected
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PairVerificationFailure {
    Source,
    Target,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CopyValueFailure {
    Read,
    Write,
}

fn canary_is_valid(backend: &dyn StorageBackend, kind: AdminStoreBackendKind) -> bool {
    if kind == AdminStoreBackendKind::Plain {
        return true;
    }
    let Ok(Some(value)) = backend.read(underwrap_backend_store::ENCRYPTED_CANARY_ID) else {
        return false;
    };
    let valid = value.as_ref() == underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT;
    drop(value);
    valid
}

fn update_fingerprint_length(
    hasher: &mut Sha256,
    length: usize,
) -> Result<(), SourceGenerationFingerprintError> {
    let length = u64::try_from(length).map_err(|_| SourceGenerationFingerprintError)?;
    hasher.update(length.to_be_bytes());
    Ok(())
}

fn fingerprint_source_generation(
    source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    ids: &BTreeSet<String>,
) -> Result<SourceGenerationFingerprint, SourceGenerationFingerprintError> {
    let mut expected_ids = ids.clone();
    if source_kind == AdminStoreBackendKind::Encrypted {
        expected_ids.insert(underwrap_backend_store::ENCRYPTED_CANARY_ID.to_string());
    }
    let actual_ids = source
        .list("")
        .map_err(|_| SourceGenerationFingerprintError)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    if actual_ids != expected_ids || !canary_is_valid(source, source_kind) {
        return Err(SourceGenerationFingerprintError);
    }

    let mut hasher = Sha256::new();
    hasher.update(SOURCE_GENERATION_FINGERPRINT_DOMAIN);
    hasher.update(b"backend-kind\0");
    hasher.update([match source_kind {
        AdminStoreBackendKind::Plain => 0,
        AdminStoreBackendKind::Encrypted => 1,
    }]);
    hasher.update(b"canary-treatment\0");
    hasher.update([u8::from(source_kind == AdminStoreBackendKind::Encrypted)]);
    hasher.update(b"storage-id-count\0");
    update_fingerprint_length(&mut hasher, expected_ids.len())?;

    for id in expected_ids {
        let value = source
            .read(&id)
            .map_err(|_| SourceGenerationFingerprintError)?
            .ok_or(SourceGenerationFingerprintError)?;
        hasher.update(b"storage-id\0");
        update_fingerprint_length(&mut hasher, id.len())?;
        hasher.update(id.as_bytes());
        hasher.update(b"storage-value\0");
        update_fingerprint_length(&mut hasher, value.as_ref().len())?;
        hasher.update(value.as_ref());
        drop(value);
    }

    Ok(SourceGenerationFingerprint(hasher.finalize().into()))
}

fn source_generation_matches(
    source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    inventory: &SourceInventory,
) -> bool {
    fingerprint_source_generation(source, source_kind, &inventory.ids)
        .is_ok_and(|fingerprint| fingerprint == inventory.fingerprint)
}

fn collect_source_inventory(
    source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    workspace: &Path,
    hooks: &dyn MigrationHooks,
) -> Result<SourceInventory, MigrationFailure> {
    if hooks.checkpoint(MigrationPhase::ListSource).is_err() {
        return Err(stage_failure(
            workspace,
            hooks,
            "migrate_list_failed",
            "could not list the migration source",
            "list-failed",
        ));
    }
    let mut ids = source
        .list("")
        .map_err(|_| {
            stage_failure(
                workspace,
                hooks,
                "migrate_list_failed",
                "could not list the migration source",
                "list-failed",
            )
        })?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let contains_canary = ids.remove(underwrap_backend_store::ENCRYPTED_CANARY_ID);
    if hooks.checkpoint(MigrationPhase::ValidateCanary).is_err()
        || (source_kind == AdminStoreBackendKind::Plain && contains_canary)
        || (source_kind == AdminStoreBackendKind::Encrypted
            && (!contains_canary || !canary_is_valid(source, source_kind)))
    {
        return Err(stage_failure(
            workspace,
            hooks,
            "migrate_canary_failed",
            "the migration source canary did not verify",
            "canary-failed",
        ));
    }

    let has_secrets = ids
        .iter()
        .any(|id| !underwrap_protocol::control::is_control_key(id));
    let fingerprint = fingerprint_source_generation(source, source_kind, &ids)
        .map_err(|_| recovery("source-fingerprint-capture-failed"))?;
    Ok(SourceInventory {
        ids,
        has_secrets,
        fingerprint,
    })
}

fn copy_one_value(
    source: &dyn StorageBackend,
    target: &dyn StorageBackend,
    id: &str,
    hooks: &dyn MigrationHooks,
) -> Result<(), CopyValueFailure> {
    if hooks.checkpoint(MigrationPhase::ReadSource).is_err() {
        return Err(CopyValueFailure::Read);
    }
    let value = source
        .read(id)
        .map_err(|_| CopyValueFailure::Read)?
        .ok_or(CopyValueFailure::Read)?;
    let written = hooks.checkpoint(MigrationPhase::WriteDestination).is_ok()
        && target.write(id, value.as_ref()).is_ok();
    drop(value);
    if written {
        Ok(())
    } else {
        Err(CopyValueFailure::Write)
    }
}

fn compare_one_value(
    source: &dyn StorageBackend,
    target: &dyn StorageBackend,
    id: &str,
) -> Result<(), PairVerificationFailure> {
    let source_value = source
        .read(id)
        .map_err(|_| PairVerificationFailure::Source)?
        .ok_or(PairVerificationFailure::Source)?;
    let target_value = match target.read(id) {
        Ok(Some(value)) => value,
        Ok(None) | Err(_) => {
            drop(source_value);
            return Err(PairVerificationFailure::Target);
        }
    };
    let equal = source_value.as_ref() == target_value.as_ref();
    drop(target_value);
    drop(source_value);
    if equal {
        Ok(())
    } else {
        Err(PairVerificationFailure::Target)
    }
}

fn verify_pair(
    source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    target: &dyn StorageBackend,
    target_kind: AdminStoreBackendKind,
    inventory: &SourceInventory,
    _hooks: &dyn MigrationHooks,
) -> Result<(), PairVerificationFailure> {
    if !source_generation_matches(source, source_kind, inventory) {
        return Err(PairVerificationFailure::Source);
    }

    let target_ids = target
        .list("")
        .map_err(|_| PairVerificationFailure::Target)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    if target_ids != inventory.expected_ids(target_kind) || !canary_is_valid(target, target_kind) {
        return Err(PairVerificationFailure::Target);
    }

    for id in &inventory.ids {
        #[cfg(test)]
        _hooks.before_compare_value_pair();
        compare_one_value(source, target, id)?;
        #[cfg(test)]
        _hooks.compared_value_pair_released();
    }
    Ok(())
}

fn reopen_source_generation(
    template: &dyn StorageBackend,
    path: &Path,
    source_kind: AdminStoreBackendKind,
    inventory: &SourceInventory,
) -> Result<Box<dyn StorageBackend>, PairVerificationFailure> {
    require_directory(path, false).map_err(|_| PairVerificationFailure::Source)?;
    let reopened = template
        .reopen_at(path)
        .map_err(|_| PairVerificationFailure::Source)?;
    if !source_generation_matches(reopened.as_ref(), source_kind, inventory) {
        return Err(PairVerificationFailure::Source);
    }
    Ok(reopened)
}

struct RollbackContext<'a> {
    canonical: &'a Path,
    stage: &'a Path,
    backup: &'a Path,
    workspace: &'a Path,
    hooks: &'a dyn MigrationHooks,
}

fn restore_unpromoted_source(
    context: &RollbackContext<'_>,
    backup_source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    target_kind: AdminStoreBackendKind,
    inventory: &SourceInventory,
    staged_target: Option<&dyn StorageBackend>,
) -> Result<(), MigrationFailure> {
    if !source_generation_matches(backup_source, source_kind, inventory) {
        return Err(recovery("rollback-source-verify-failed"));
    }
    if let Some(target) = staged_target {
        match verify_pair(
            backup_source,
            source_kind,
            target,
            target_kind,
            inventory,
            context.hooks,
        ) {
            Ok(()) | Err(PairVerificationFailure::Target) => {}
            Err(PairVerificationFailure::Source) => {
                return Err(recovery("rollback-source-verify-failed"));
            }
        }
    }
    context
        .hooks
        .rename(
            MigrationPhase::RestoreBackup,
            context.backup,
            context.canonical,
        )
        .map_err(|_| recovery("rollback-failed"))?;
    reopen_source_generation(backup_source, context.canonical, source_kind, inventory)
        .map_err(|_| recovery("rollback-verify-failed"))?;
    Ok(())
}

fn restore_source_generation(
    context: &RollbackContext<'_>,
    backup_source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    target_kind: AdminStoreBackendKind,
    inventory: &SourceInventory,
    exact_target: Option<&dyn StorageBackend>,
) -> Result<(), MigrationFailure> {
    if !source_generation_matches(backup_source, source_kind, inventory) {
        return Err(recovery("rollback-source-verify-failed"));
    }
    if let Some(target) = exact_target {
        verify_pair(
            backup_source,
            source_kind,
            target,
            target_kind,
            inventory,
            context.hooks,
        )
        .map_err(|_| recovery("rollback-pair-verify-failed"))?;
    }
    context
        .hooks
        .rename(
            MigrationPhase::MovePromotedAside,
            context.canonical,
            context.stage,
        )
        .map_err(|_| recovery("rollback-failed"))?;
    if !source_generation_matches(backup_source, source_kind, inventory) {
        return Err(recovery("rollback-source-verify-failed"));
    }
    context
        .hooks
        .rename(
            MigrationPhase::RestoreBackup,
            context.backup,
            context.canonical,
        )
        .map_err(|_| recovery("rollback-failed"))?;
    reopen_source_generation(backup_source, context.canonical, source_kind, inventory)
        .map_err(|_| recovery("rollback-verify-failed"))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn rollback_promoted(
    context: &RollbackContext<'_>,
    backup_source: &dyn StorageBackend,
    source_kind: AdminStoreBackendKind,
    target_kind: AdminStoreBackendKind,
    inventory: &SourceInventory,
    exact_target: Option<&dyn StorageBackend>,
) -> Result<(), MigrationFailure> {
    restore_source_generation(
        context,
        backup_source,
        source_kind,
        target_kind,
        inventory,
        exact_target,
    )?;
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

    let inventory = collect_source_inventory(source, source_kind, &workspace, hooks)?;
    if target_kind == AdminStoreBackendKind::Encrypted
        && (hooks.checkpoint(MigrationPhase::ValidateCanary).is_err()
            || staged
                .write(
                    underwrap_backend_store::ENCRYPTED_CANARY_ID,
                    underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
                )
                .is_err()
            || !canary_is_valid(staged.as_ref(), target_kind))
    {
        return Err(stage_failure_with_verified_source(
            source,
            source_kind,
            &inventory,
            &workspace,
            hooks,
            "migrate_canary_failed",
            "the migration target canary did not verify",
            "canary-failed",
        ));
    }

    for id in &inventory.ids {
        match copy_one_value(source, staged.as_ref(), id, hooks) {
            Ok(()) => {
                #[cfg(test)]
                hooks.copied_value_released();
            }
            Err(CopyValueFailure::Read) => {
                return Err(stage_failure_with_verified_source(
                    source,
                    source_kind,
                    &inventory,
                    &workspace,
                    hooks,
                    "migrate_read_failed",
                    "could not read a migration source value",
                    "read-failed",
                ));
            }
            Err(CopyValueFailure::Write) => {
                return Err(stage_failure_with_verified_source(
                    source,
                    source_kind,
                    &inventory,
                    &workspace,
                    hooks,
                    "migrate_write_failed",
                    "could not write a migration target value",
                    "write-failed",
                ));
            }
        }
    }
    if hooks.checkpoint(MigrationPhase::VerifyStage).is_err() {
        return Err(stage_failure_with_verified_source(
            source,
            source_kind,
            &inventory,
            &workspace,
            hooks,
            "migrate_verify_failed",
            "the staged migration target did not verify",
            "verify-failed",
        ));
    }
    match verify_pair(
        source,
        source_kind,
        staged.as_ref(),
        target_kind,
        &inventory,
        hooks,
    ) {
        Ok(()) => {}
        Err(PairVerificationFailure::Source) => {
            return Err(recovery("source-fingerprint-verify-recovery-required"));
        }
        Err(PairVerificationFailure::Target) => {
            return Err(stage_failure_with_verified_source(
                source,
                source_kind,
                &inventory,
                &workspace,
                hooks,
                "migrate_verify_failed",
                "the staged migration target did not verify",
                "verify-failed",
            ));
        }
    }

    if hooks
        .rename(MigrationPhase::DisplaceLive, canonical, &backup)
        .is_err()
    {
        if require_directory(canonical, false).is_err()
            || !source_generation_matches(source, source_kind, &inventory)
        {
            return Err(recovery("swap-recovery-required"));
        }
        if matches!(
            verify_pair(
                source,
                source_kind,
                staged.as_ref(),
                target_kind,
                &inventory,
                hooks,
            ),
            Err(PairVerificationFailure::Source)
        ) {
            return Err(recovery("swap-recovery-required"));
        }
        return Err(stage_failure(
            &workspace,
            hooks,
            "migrate_swap_failed",
            "could not displace the migration source",
            "swap-failed",
        ));
    }

    let backup_source = reopen_source_generation(source, &backup, source_kind, &inventory)
        .map_err(|_| recovery("backup-reopen-recovery-required"))?;
    match verify_pair(
        backup_source.as_ref(),
        source_kind,
        staged.as_ref(),
        target_kind,
        &inventory,
        hooks,
    ) {
        Ok(()) => {}
        Err(PairVerificationFailure::Source) => {
            return Err(recovery("backup-verify-recovery-required"));
        }
        Err(PairVerificationFailure::Target) => {
            let rollback = RollbackContext {
                canonical,
                stage: &stage,
                backup: &backup,
                workspace: &workspace,
                hooks,
            };
            restore_unpromoted_source(
                &rollback,
                backup_source.as_ref(),
                source_kind,
                target_kind,
                &inventory,
                None,
            )?;
            cleanup_workspace(&workspace, hooks).map_err(|_| recovery("cleanup-failed"))?;
            return Err(failure(
                "migrate_verify_failed",
                "the staged migration target did not verify; the source was restored",
                "verify-failed",
            ));
        }
    }

    if hooks
        .rename(MigrationPhase::PromoteStage, &stage, canonical)
        .is_err()
    {
        let rollback = RollbackContext {
            canonical,
            stage: &stage,
            backup: &backup,
            workspace: &workspace,
            hooks,
        };
        restore_unpromoted_source(
            &rollback,
            backup_source.as_ref(),
            source_kind,
            target_kind,
            &inventory,
            Some(staged.as_ref()),
        )?;
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
        rollback_promoted(
            &rollback,
            backup_source.as_ref(),
            source_kind,
            target_kind,
            &inventory,
            None,
        )?;
        return Err(failure(
            "migrate_reopen_failed",
            "could not reopen the migration target; the source was restored",
            "reopen-failed",
        ));
    }
    let promoted = match reopen_target(canonical, target_kind, target_key) {
        Ok(backend) => backend,
        Err(_) => {
            rollback_promoted(
                &rollback,
                backup_source.as_ref(),
                source_kind,
                target_kind,
                &inventory,
                None,
            )?;
            return Err(failure(
                "migrate_reopen_failed",
                "could not reopen the migration target; the source was restored",
                "reopen-failed",
            ));
        }
    };
    if hooks.checkpoint(MigrationPhase::VerifyPromoted).is_err() {
        rollback_promoted(
            &rollback,
            backup_source.as_ref(),
            source_kind,
            target_kind,
            &inventory,
            Some(promoted.as_ref()),
        )?;
        return Err(failure(
            "migrate_verify_failed",
            "the promoted migration target did not verify; the source was restored",
            "verify-failed",
        ));
    }
    match verify_pair(
        backup_source.as_ref(),
        source_kind,
        promoted.as_ref(),
        target_kind,
        &inventory,
        hooks,
    ) {
        Ok(()) => {}
        Err(PairVerificationFailure::Source) => {
            return Err(recovery("promoted-source-verify-recovery-required"));
        }
        Err(PairVerificationFailure::Target) => {
            rollback_promoted(
                &rollback,
                backup_source.as_ref(),
                source_kind,
                target_kind,
                &inventory,
                None,
            )?;
            return Err(failure(
                "migrate_verify_failed",
                "the promoted migration target did not verify; the source was restored",
                "verify-failed",
            ));
        }
    }

    if hooks.checkpoint(MigrationPhase::PersistRecord).is_err() || persist_target().is_err() {
        restore_source_generation(
            &rollback,
            backup_source.as_ref(),
            source_kind,
            target_kind,
            &inventory,
            Some(promoted.as_ref()),
        )?;
        drop(promoted);
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
    Ok(MigrationSuccess {
        backend: promoted,
        has_secrets: inventory.has_secrets,
    })
}

#[cfg(test)]
mod tests;
