use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use underwrap_backend_store::{EncryptedBackend, StorageBackend};

use crate::admin_api::AdminError;
use crate::admin_api::stores::{generate_master_key_bytes, master_key_hex};

use super::workspace::{checked_rename, create_private_directory};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum RotationPhase {
    ReserveWorkspace,
    OpenStage,
    ListSource,
    WriteCanary,
    ReadSource,
    WriteDestination,
    VerifyStage,
    DisplaceLive,
    PromoteStage,
    OpenPromoted,
    VerifyPromoted,
    MovePromotedAside,
    RestoreBackup,
}

pub(super) trait RotationHooks: Sync {
    fn rotation_id(&self) -> String;

    fn replacement_key(&self) -> [u8; 32] {
        generate_master_key_bytes()
    }

    fn checkpoint(&self, _phase: RotationPhase) -> io::Result<()> {
        Ok(())
    }

    fn rename(&self, phase: RotationPhase, from: &Path, to: &Path) -> io::Result<()> {
        self.checkpoint(phase)?;
        checked_rename(from, to)
    }

    fn before_rotation_lock(&self) {}

    fn before_publish(&self) {}
}

pub(super) struct ProductionHooks;

impl RotationHooks for ProductionHooks {
    fn rotation_id(&self) -> String {
        crate::ids::random_hex_8()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FailureDisposition {
    KeepUnlocked,
    Lock,
}

#[derive(Debug)]
pub(super) struct RotationFailure {
    pub(super) error: AdminError,
    pub(super) disposition: FailureDisposition,
    pub(super) audit_reason: &'static str,
}

fn clear_owned_string(value: String) {
    let mut bytes = value.into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
    #[cfg(test)]
    KEY_HEX_WIPE_WITNESS.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
thread_local! {
    static KEY_HEX_WIPE_WITNESS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn key_hex_wipe_count() -> usize {
    KEY_HEX_WIPE_WITNESS.with(std::cell::Cell::get)
}

struct GeneratedKeyHex(Option<String>);

impl GeneratedKeyHex {
    fn new(value: String) -> Self {
        Self(Some(value))
    }

    fn as_str(&self) -> &str {
        self.0.as_deref().expect("generated key hex is present")
    }

    fn into_string(mut self) -> String {
        self.0.take().expect("generated key hex is present")
    }
}

impl Drop for GeneratedKeyHex {
    fn drop(&mut self) {
        if let Some(value) = self.0.take() {
            clear_owned_string(value);
        }
    }
}

pub(super) struct RotationSuccess {
    pub(super) backend: Arc<dyn StorageBackend>,
    pub(super) key_hex: String,
}

impl RotationSuccess {
    pub(super) fn discard(self) {
        let Self { backend, key_hex } = self;
        drop(backend);
        clear_owned_string(key_hex);
    }
}

impl std::fmt::Debug for RotationSuccess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RotationSuccess")
            .field("backend", &"<encrypted backend>")
            .field("key_hex", &"<redacted>")
            .finish()
    }
}

fn failure(code: &'static str, message: &'static str, reason: &'static str) -> RotationFailure {
    RotationFailure {
        error: AdminError::internal_with_code(code, message),
        disposition: FailureDisposition::KeepUnlocked,
        audit_reason: reason,
    }
}

fn recovery_required(reason: &'static str) -> RotationFailure {
    RotationFailure {
        error: AdminError::rotation_recovery_required(),
        disposition: FailureDisposition::Lock,
        audit_reason: reason,
    }
}

fn reserve_workspace(
    parent: &Path,
    store_id: &str,
    hooks: &dyn RotationHooks,
) -> Result<PathBuf, RotationFailure> {
    for _ in 0..32 {
        hooks
            .checkpoint(RotationPhase::ReserveWorkspace)
            .map_err(|_| {
                failure(
                    "rotate_stage_failed",
                    "could not reserve a rotation staging directory",
                    "stage-failed",
                )
            })?;
        let id = hooks.rotation_id();
        if id.len() != 16
            || !id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(failure(
                "rotate_stage_failed",
                "rotation identifier generation failed",
                "stage-failed",
            ));
        }
        let workspace = parent.join(format!(".{store_id}.underwrap-rotation-{id}"));
        match create_private_directory(&workspace) {
            Ok(()) => return Ok(workspace),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                return Err(failure(
                    "rotate_stage_failed",
                    "could not reserve a rotation staging directory",
                    "stage-failed",
                ));
            }
        }
    }
    Err(failure(
        "rotate_stage_failed",
        "could not allocate a unique rotation staging directory",
        "stage-failed",
    ))
}

fn canonical_generation_is_usable(canonical: &Path, current_backend: &dyn StorageBackend) -> bool {
    matches!(
        std::fs::symlink_metadata(canonical),
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink()
    ) && current_backend.list("").is_ok()
        && matches!(
            current_backend.read(underwrap_backend_store::ENCRYPTED_CANARY_ID),
            Ok(Some(ref value))
                if value.as_ref() == underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT
        )
}

fn rollback_after_promotion(
    canonical: &Path,
    stage: &Path,
    backup: &Path,
    hooks: &dyn RotationHooks,
) -> Result<(), RotationFailure> {
    hooks
        .rename(RotationPhase::MovePromotedAside, canonical, stage)
        .map_err(|_| recovery_required("rollback-failed"))?;
    hooks
        .rename(RotationPhase::RestoreBackup, backup, canonical)
        .map_err(|_| recovery_required("rollback-failed"))
}

pub(super) fn rotate_generation(
    store_id: &str,
    canonical: &Path,
    current_backend: &dyn StorageBackend,
    hooks: &dyn RotationHooks,
) -> Result<RotationSuccess, RotationFailure> {
    let parent = canonical.parent().ok_or_else(|| {
        failure(
            "rotate_stage_failed",
            "encrypted store directory has no parent",
            "stage-failed",
        )
    })?;
    let workspace = reserve_workspace(parent, store_id, hooks)?;
    let stage = workspace.join("stage");
    let backup = workspace.join("backup");
    let cleanup_incomplete = || {
        let _ = std::fs::remove_dir_all(&workspace);
    };

    if hooks.checkpoint(RotationPhase::OpenStage).is_err() {
        cleanup_incomplete();
        return Err(failure(
            "rotate_stage_failed",
            "could not open the rotation staging generation",
            "stage-failed",
        ));
    }
    let mut new_key = hooks.replacement_key();
    let key_hex = GeneratedKeyHex::new(master_key_hex(&new_key));
    let staged_result = EncryptedBackend::create_with_key(&stage, new_key);
    new_key.fill(0);
    let staged = match staged_result {
        Ok(backend) => backend,
        Err(_) => {
            cleanup_incomplete();
            return Err(failure(
                "rotate_stage_failed",
                "could not open the rotation staging generation",
                "stage-failed",
            ));
        }
    };

    if hooks.checkpoint(RotationPhase::ListSource).is_err() {
        cleanup_incomplete();
        return Err(failure(
            "rotate_list_failed",
            "could not list the encrypted generation for rotation",
            "list-failed",
        ));
    }
    let source_ids = match current_backend.list("") {
        Ok(ids) => ids,
        Err(_) => {
            cleanup_incomplete();
            return Err(failure(
                "rotate_list_failed",
                "could not list the encrypted generation for rotation",
                "list-failed",
            ));
        }
    };

    if hooks.checkpoint(RotationPhase::WriteCanary).is_err()
        || staged
            .write(
                underwrap_backend_store::ENCRYPTED_CANARY_ID,
                underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
            )
            .is_err()
    {
        cleanup_incomplete();
        return Err(failure(
            "rotate_canary_failed",
            "could not write the staged rotation canary",
            "canary-write-failed",
        ));
    }

    for id in &source_ids {
        if id == underwrap_backend_store::ENCRYPTED_CANARY_ID {
            continue;
        }
        if hooks.checkpoint(RotationPhase::ReadSource).is_err() {
            cleanup_incomplete();
            return Err(failure(
                "rotate_read_failed",
                "could not read an encrypted value for rotation",
                "read-failed",
            ));
        }
        let material = match current_backend.read(id) {
            Ok(Some(material)) => material,
            Ok(None) => continue,
            Err(_) => {
                cleanup_incomplete();
                return Err(failure(
                    "rotate_read_failed",
                    "could not read an encrypted value for rotation",
                    "read-failed",
                ));
            }
        };
        if hooks.checkpoint(RotationPhase::WriteDestination).is_err()
            || staged.write(id, material.as_ref()).is_err()
        {
            cleanup_incomplete();
            return Err(failure(
                "rotate_write_failed",
                "could not write an encrypted value into the staged generation",
                "write-failed",
            ));
        }
        if hooks.checkpoint(RotationPhase::VerifyStage).is_err()
            || !matches!(staged.read(id), Ok(Some(ref value)) if value.as_ref() == material.as_ref())
        {
            cleanup_incomplete();
            return Err(failure(
                "rotate_verify_failed",
                "the staged rotation generation did not verify",
                "verify-failed",
            ));
        }
    }

    let mut expected: BTreeSet<String> = source_ids.into_iter().collect();
    expected.insert(underwrap_backend_store::ENCRYPTED_CANARY_ID.to_string());
    let staged_ids: BTreeSet<String> = match staged.list("") {
        Ok(ids) => ids.into_iter().collect(),
        Err(_) => {
            cleanup_incomplete();
            return Err(failure(
                "rotate_verify_failed",
                "the staged rotation generation did not verify",
                "verify-failed",
            ));
        }
    };
    if staged_ids != expected
        || !matches!(
            staged.read(underwrap_backend_store::ENCRYPTED_CANARY_ID),
            Ok(Some(ref value)) if value.as_ref() == underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT
        )
    {
        cleanup_incomplete();
        return Err(failure(
            "rotate_verify_failed",
            "the staged rotation generation did not verify",
            "verify-failed",
        ));
    }

    if hooks
        .rename(RotationPhase::DisplaceLive, canonical, &backup)
        .is_err()
    {
        if canonical_generation_is_usable(canonical, current_backend) {
            cleanup_incomplete();
            return Err(failure(
                "rotate_swap_failed",
                "could not displace the active encrypted generation",
                "swap-failed",
            ));
        }
        return Err(recovery_required("swap-recovery-required"));
    }

    if hooks
        .rename(RotationPhase::PromoteStage, &stage, canonical)
        .is_err()
    {
        if hooks
            .rename(RotationPhase::RestoreBackup, &backup, canonical)
            .is_err()
        {
            return Err(recovery_required("rollback-failed"));
        }
        return Err(failure(
            "rotate_swap_failed",
            "could not promote the staged encrypted generation; the previous generation was restored",
            "swap-failed",
        ));
    }

    drop(staged);
    let mut reopen_key = match underwrap_backend_store::parse_hex_key(key_hex.as_str()) {
        Ok(key) => key,
        Err(_) => {
            rollback_after_promotion(canonical, &stage, &backup, hooks)?;
            return Err(failure(
                "rotate_reopen_failed",
                "could not reopen the promoted encrypted generation; the previous generation was restored",
                "reopen-failed",
            ));
        }
    };
    if hooks.checkpoint(RotationPhase::OpenPromoted).is_err() {
        reopen_key.fill(0);
        rollback_after_promotion(canonical, &stage, &backup, hooks)?;
        return Err(failure(
            "rotate_reopen_failed",
            "could not reopen the promoted encrypted generation; the previous generation was restored",
            "reopen-failed",
        ));
    }
    let rotated_result = EncryptedBackend::open_existing_with_key(canonical, reopen_key);
    reopen_key.fill(0);
    let rotated = match rotated_result {
        Ok(backend) => backend,
        Err(_) => {
            rollback_after_promotion(canonical, &stage, &backup, hooks)?;
            return Err(failure(
                "rotate_reopen_failed",
                "could not reopen the promoted encrypted generation; the previous generation was restored",
                "reopen-failed",
            ));
        }
    };
    if hooks.checkpoint(RotationPhase::VerifyPromoted).is_err()
        || !matches!(
            rotated.read(underwrap_backend_store::ENCRYPTED_CANARY_ID),
            Ok(Some(ref value)) if value.as_ref() == underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT
        )
    {
        drop(rotated);
        rollback_after_promotion(canonical, &stage, &backup, hooks)?;
        return Err(failure(
            "rotate_reopen_failed",
            "the promoted encrypted generation did not verify; the previous generation was restored",
            "reopen-failed",
        ));
    }

    Ok(RotationSuccess {
        backend: Arc::new(rotated),
        key_hex: key_hex.into_string(),
    })
}

#[cfg(test)]
mod tests;
