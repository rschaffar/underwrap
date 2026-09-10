//! Store admin CRUD + lifecycle: `/v1/admin/stores/*`.
//!
//! Covers store CRUD, generated keys, backend migration, unlocking, locking,
//! and key rotation.
//! These handlers share registry, audit, data-directory, key-material, usage,
//! and action-projection helpers.
//!
//! Store-record writes use atomic replacement and a collection mutation lock.
//! The live registry lock is held only for admission, revalidation, and
//! publication, never for storage or record I/O.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use underwrap_protocol::service::{
    AdminStoreActionState, AdminStoreBackendKind, AdminStoreLifecycleState, AdminStoreListItem,
    AdminStoreUsageSummary,
};

use crate::admin_api::AdminError;
use crate::runtime_state::ListenerState;
use crate::store_registry::RegistryLookupError;
use underwrap_protocol::config::StoreRecord;

pub(crate) mod crud;
pub(crate) mod lifecycle;

/// Resolve a store reference for an authenticated admin session.
/// Admin sessions are unconditionally trusted to address any store;
/// per-store wrapper grants gate *wrapper* access, not admin access.
/// Returns the live backend or the canonical not-found / locked
/// [`AdminError`].
pub(crate) fn admin_store(
    state: &ListenerState,
    store: &str,
) -> std::result::Result<crate::store_registry::StoreLease, AdminError> {
    match state.shared.stores.read().unwrap().lease(store) {
        Ok(lease) => Ok(lease),
        Err(RegistryLookupError::Locked) => Err(AdminError::store_locked(store)),
        Err(RegistryLookupError::Busy) => Err(AdminError::store_busy()),
        Err(RegistryLookupError::RecoveryRequired) => {
            Err(AdminError::migration_recovery_required())
        }
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

fn mapped_client_instance_count(state: &ListenerState, store_id: &str) -> u32 {
    crate::admin_api::util::saturating_u32(
        state
            .shared
            .client_instances
            .read()
            .unwrap()
            .values()
            .filter(|instance| instance.current_store_id == store_id)
            .count(),
    )
}

pub(super) fn admin_store_usage(
    state: &ListenerState,
    store_id: &str,
    store: &dyn underwrap_backend_store::StorageBackend,
) -> std::result::Result<AdminStoreUsageSummary, AdminError> {
    let mapped_client_instance_count = mapped_client_instance_count(state, store_id);
    let has_secrets = store
        .list("")
        .map_err(|e| {
            AdminError::internal_with_source(
                "list_store_failed",
                "could not inspect store contents",
                "checking store contents for management view",
                e,
            )
        })?
        .iter()
        .any(|id| !underwrap_protocol::control::is_control_key(id));
    Ok(AdminStoreUsageSummary {
        mapped_client_instance_count,
        has_secrets,
    })
}

pub(super) fn admin_store_actions(
    lifecycle: AdminStoreLifecycleState,
    usage: &AdminStoreUsageSummary,
) -> AdminStoreActionState {
    let unmapped = usage.mapped_client_instance_count == 0;
    match lifecycle {
        AdminStoreLifecycleState::Ready => AdminStoreActionState {
            can_update: true,
            can_migrate: true,
            can_rotate_key: false,
            can_unlock: false,
            can_lock: false,
            can_delete_now: unmapped && !usage.has_secrets,
            can_force_delete: unmapped && usage.has_secrets,
        },
        AdminStoreLifecycleState::Unlocked => AdminStoreActionState {
            can_update: true,
            can_migrate: true,
            can_rotate_key: true,
            can_unlock: false,
            can_lock: true,
            can_delete_now: unmapped && !usage.has_secrets,
            can_force_delete: unmapped && usage.has_secrets,
        },
        AdminStoreLifecycleState::Locked => AdminStoreActionState {
            can_update: true,
            can_migrate: false,
            can_rotate_key: false,
            can_unlock: true,
            can_lock: false,
            can_delete_now: false,
            can_force_delete: unmapped,
        },
        AdminStoreLifecycleState::Busy | AdminStoreLifecycleState::RecoveryRequired => {
            AdminStoreActionState {
                can_update: false,
                can_migrate: false,
                can_rotate_key: false,
                can_unlock: false,
                can_lock: false,
                can_delete_now: false,
                can_force_delete: false,
            }
        }
    }
}

pub(super) fn admin_store_list_item(
    record: &StoreRecord,
    lifecycle_state: AdminStoreLifecycleState,
    usage: AdminStoreUsageSummary,
    actions: AdminStoreActionState,
) -> AdminStoreListItem {
    AdminStoreListItem {
        id: record.id.clone(),
        name: record.name.clone(),
        backend: backend_kind(&record.storage),
        lifecycle_state,
        description: record.description.clone(),
        created_at: record.created_at,
        usage,
        actions,
    }
}

pub(super) fn lifecycle_state(
    state: &crate::store_registry::StoreState,
) -> AdminStoreLifecycleState {
    match state {
        crate::store_registry::StoreState::Ready(_) => AdminStoreLifecycleState::Ready,
        crate::store_registry::StoreState::Unlocked(_) => AdminStoreLifecycleState::Unlocked,
        crate::store_registry::StoreState::Locked => AdminStoreLifecycleState::Locked,
        crate::store_registry::StoreState::Busy => AdminStoreLifecycleState::Busy,
        crate::store_registry::StoreState::RecoveryRequired => {
            AdminStoreLifecycleState::RecoveryRequired
        }
    }
}

/// Usage summary when the backend is unavailable for content inspection.
/// `has_secrets: false` is a placeholder, not proof of an empty store.
/// Lifecycle state and server-derived actions keep locked deletion conservative
/// and prohibit deletion while busy or recovery-required.
pub(super) fn admin_store_usage_for_locked(
    state: &ListenerState,
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
    state: &ListenerState,
    store_id: &str,
) -> std::result::Result<PathBuf, (&'static str, String)> {
    let Some(root) = state.admin().stores_data_root.as_ref() else {
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

/// Generate 32 fresh random bytes via the OS RNG. Constructors copy the
/// fixed-size array into an [`EncryptedBackend`], which zeroizes its copy on
/// drop; callers wipe their temporary binary copy immediately afterward. The
/// deliberately retained lowercase-hex response remains live only until its
/// one-time JSON response is returned or discarded.
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

pub(crate) fn openapi_router() -> utoipa_axum::router::OpenApiRouter<Arc<ListenerState>> {
    use utoipa_axum::router::OpenApiRouter;
    OpenApiRouter::new()
        .merge(crud::openapi_router())
        .merge(lifecycle::openapi_router())
}

#[cfg(test)]
mod tests;
