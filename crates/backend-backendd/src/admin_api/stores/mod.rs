//! Store admin CRUD + lifecycle: `/v1/admin/stores/*`.
//!
//! Covers `list`, `detail`, `create`, `update`, `delete` plus the
//! three lifecycle operations `unlock`, `lock`, and `rotate-key`. CRUD
//! and lifecycle share `admin_store`, the registry helpers, the audit
//! shim, the data-dir helpers, the key-material helpers, and the
//! usage / actions projections; they sit in this module so the two
//! halves can each `use super::*;`.
//!
//! All five CRUD handlers operate on the runtime `state.stores`
//! registry plus the backend-managed store-registry file at
//! `state.stores_file`. Atomicity comes from `tempfile::NamedTempFile`
//! `persist()` on the registry write side and a brief held `write()`
//! lock on the in-memory registry update.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use underwrap_protocol::service::{
    AdminStoreActionState, AdminStoreBackendKind, AdminStoreDetailResponse, AdminStoreListItem,
    AdminStoreUsageSummary,
};
use underwrap_store::StorageBackend;

use crate::admin_api::AdminError;
use crate::admin_api::audit::log_admin_event;
use crate::runtime_state::AdminRuntimeState;
use crate::store_registry::{LoadedStoreRecord, RegistryLookupError};

pub(crate) mod crud;
pub(crate) mod lifecycle;

/// Resolve a store reference for an authenticated request. Any
/// authenticated user can address any store; the helper returns the
/// live backend or the canonical not-found / locked [`AdminError`].
pub(crate) fn admin_store(
    state: &AdminRuntimeState,
    store: &str,
) -> std::result::Result<Arc<dyn StorageBackend>, AdminError> {
    match state.stores.read().unwrap().unlocked_backend(store) {
        Ok(s) => Ok(s),
        Err(RegistryLookupError::Locked) => Err(AdminError::store_locked(store)),
        Err(RegistryLookupError::NotFound) => Err(AdminError::store_not_found()),
    }
}

pub(super) fn backend_kind(
    storage: &underwrap_protocol::config::StorageConfig,
) -> AdminStoreBackendKind {
    match storage {
        underwrap_protocol::config::StorageConfig::Plain { .. } => AdminStoreBackendKind::Plain,
        underwrap_protocol::config::StorageConfig::Encrypted { .. } => {
            AdminStoreBackendKind::Encrypted
        }
    }
}

fn saturating_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

fn mapped_client_instance_count(state: &AdminRuntimeState, store_id: &str) -> u32 {
    saturating_u32(
        state
            .client_instances
            .read()
            .unwrap()
            .values()
            .filter(|instance| instance.current_store_id == store_id)
            .count(),
    )
}

pub(super) fn admin_store_usage(
    state: &AdminRuntimeState,
    store_id: &str,
    store: &dyn underwrap_store::StorageBackend,
) -> std::result::Result<AdminStoreUsageSummary, AdminError> {
    let mapped_client_instance_count = mapped_client_instance_count(state, store_id);
    let has_secrets = store
        .list("")
        .map_err(|e| {
            AdminError::internal_with_code(
                "list_store_failed",
                format!("checking store contents for management view: {e:#}"),
            )
        })?
        .iter()
        .any(|id| id != underwrap_store::ENCRYPTED_CANARY_ID);
    Ok(AdminStoreUsageSummary {
        mapped_client_instance_count,
        has_secrets,
    })
}

pub(super) fn admin_store_actions(
    loaded: &LoadedStoreRecord,
    usage: &AdminStoreUsageSummary,
    locked: bool,
) -> AdminStoreActionState {
    let is_encrypted = matches!(
        &loaded.record.storage,
        underwrap_protocol::config::StorageConfig::Encrypted { .. }
    );
    // Delete works for both locked and unlocked stores — we use the
    // `has_secrets` flag only to choose between plain DELETE and
    // force-DELETE. For locked stores we cannot inspect contents, so
    // we conservatively offer force-DELETE only.
    let can_delete_without_force =
        usage.mapped_client_instance_count == 0 && !usage.has_secrets && !locked;
    let can_force_delete = usage.mapped_client_instance_count == 0 && (usage.has_secrets || locked);
    AdminStoreActionState {
        can_edit_metadata: true,
        can_rotate_key: is_encrypted && !locked,
        can_unlock: is_encrypted && locked,
        can_lock: is_encrypted && !locked,
        can_delete_now: can_delete_without_force,
        can_force_delete,
    }
}

pub(super) fn admin_store_list_item(
    loaded: &LoadedStoreRecord,
    locked: bool,
    usage: AdminStoreUsageSummary,
    actions: AdminStoreActionState,
) -> AdminStoreListItem {
    AdminStoreListItem {
        id: loaded.record.id.clone(),
        name: loaded.record.name.clone(),
        backend: backend_kind(&loaded.record.storage),
        description: loaded.record.description.clone(),
        created_at: loaded.record.created_at,
        locked,
        usage,
        actions,
    }
}

pub(super) fn admin_store_detail(
    stores_file: &Path,
    loaded: &LoadedStoreRecord,
    locked: bool,
    usage: AdminStoreUsageSummary,
    actions: AdminStoreActionState,
) -> AdminStoreDetailResponse {
    let dir = match &loaded.record.storage {
        underwrap_protocol::config::StorageConfig::Plain { dir }
        | underwrap_protocol::config::StorageConfig::Encrypted { dir } => {
            dir.to_string_lossy().to_string()
        }
    };
    AdminStoreDetailResponse {
        id: loaded.record.id.clone(),
        name: loaded.record.name.clone(),
        backend: backend_kind(&loaded.record.storage),
        dir,
        locked,
        description: loaded.record.description.clone(),
        created_at: loaded.record.created_at,
        source_path: stores_file.to_string_lossy().to_string(),
        usage,
        actions,
    }
}

/// Usage summary for a store that we cannot currently inspect because
/// it is locked. We surface zero secret count rather than guessing —
/// the SPA's "is this safe to delete" hint then falls into the
/// force-delete bucket and the operator gets a clear "unlock first
/// or force-delete" decision.
pub(super) fn admin_store_usage_for_locked(
    state: &AdminRuntimeState,
    store_id: &str,
) -> AdminStoreUsageSummary {
    AdminStoreUsageSummary {
        mapped_client_instance_count: mapped_client_instance_count(state, store_id),
        has_secrets: false,
    }
}

/// Resolve the data directory of a new store. The on-disk layout is
/// `<stores_data_root>/<store_id>/`; the admin only chooses the store's
/// display name, so the directory leaf is the random store id. An
/// unconfigured root disables creation entirely.
pub(crate) fn resolve_store_dir(
    state: &AdminRuntimeState,
    store_id: &str,
) -> std::result::Result<PathBuf, (&'static str, String)> {
    let Some(root) = state.stores_data_root.as_ref() else {
        return Err((
            "data_root_not_configured",
            "store creation is disabled: backend.yaml has no `admin.stores_data_root`".to_string(),
        ));
    };
    if !root.is_absolute() {
        return Err((
            "data_root_not_absolute",
            format!(
                "`admin.stores_data_root` must be an absolute path, got {}",
                root.display()
            ),
        ));
    }
    Ok(root.join(store_id))
}

/// Generate 32 fresh random bytes via the OS RNG. The bytes are
/// caller-owned and should be moved into either the [`EncryptedBackend`]
/// (which zeroizes on drop) or the JSON response buffer — never both.
pub(super) fn generate_master_key_bytes() -> [u8; 32] {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}

/// Hex-encode a 32-byte master key as 64 lowercase characters.
pub(super) fn master_key_hex(bytes: &[u8; 32]) -> String {
    hex::encode(bytes)
}

#[cfg(unix)]
pub(super) fn create_data_dir_secure(dir: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if dir.exists() {
        return Ok(());
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("creating {}", dir.display()))
}
#[cfg(not(unix))]
pub(super) fn create_data_dir_secure(dir: &Path) -> Result<()> {
    if dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))
}

pub(crate) fn write_admin_store_audit(
    principal: &crate::users::Principal,
    store_name: &str,
    action: &str,
    operation: Option<&str>,
    result: &str,
) {
    let resource = format!("store:{store_name}");
    log_admin_event(
        action,
        &principal.username,
        None,
        Some(resource.as_str()),
        None,
        operation,
        result,
    );
}

pub(crate) fn openapi_router() -> utoipa_axum::router::OpenApiRouter<Arc<AdminRuntimeState>> {
    use utoipa_axum::router::OpenApiRouter;
    OpenApiRouter::new()
        .merge(crud::openapi_router())
        .merge(lifecycle::openapi_router())
}

#[cfg(test)]
mod tests;
