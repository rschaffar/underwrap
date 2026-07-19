//! Encrypted-store lifecycle: `unlock`, `lock`, `rotate-key`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxumPath, State};
use underwrap_backend_store::StorageBackend;
use underwrap_protocol::service::{
    AdminOkResponse, AdminStoreLockResponse, AdminStoreRotateKeyResponse, AdminStoreUnlockRequest,
    ServiceErrorResponse,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::csrf::Principal;
use crate::http::responses::Body;
use crate::runtime_state::ListenerState;
use crate::store_registry::StoreState;

mod rotation;

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<ListenerState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_store_rotate_key))
        .routes(routes!(handle_admin_store_unlock))
        .routes(routes!(handle_admin_store_lock))
}

use crate::admin_api::audit::{AuditEntity, log_admin_event};

/// `POST /v1/admin/stores/{id}/rotate-key` — mint a fresh in-memory
/// master key, re-encrypt the complete store, atomically activate it, and
/// return the key once. Every displaced ciphertext generation remains in a
/// uniquely named backend-host recovery workspace until trusted manual cleanup.
/// HTTP delivery and the browser's local acknowledgement do not prove custody.
#[utoipa::path(
    post,
    path = "/v1/admin/stores/{id}/rotate-key",
    tag = "stores",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    responses(
        (status = 200, description = "Key rotated and returned once; the displaced encrypted generation is retained for backend-host recovery.", body = AdminStoreRotateKeyResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
        (status = 500, description = "Rotation failed; rotation_recovery_required means trusted host recovery is required.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_rotate_key(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<AdminStoreRotateKeyResponse>, AdminError> {
    rotate_key_with_hooks(state, principal, name, &rotation::ProductionHooks).await
}

struct RotationAdmission {
    rotation_lock: Arc<tokio::sync::Mutex<()>>,
    canonical: std::path::PathBuf,
    backend: Arc<dyn StorageBackend>,
}

fn entry_matches_admission(
    entry: &crate::store_registry::StoreEntry,
    admission: &RotationAdmission,
) -> bool {
    let same_path = matches!(
        &entry.record.storage,
        underwrap_protocol::config::StorageConfig::Encrypted { dir }
            if dir == &admission.canonical
    );
    Arc::ptr_eq(&entry.rotation_lock, &admission.rotation_lock)
        && same_path
        && matches!(
            &entry.state,
            StoreState::Unlocked(backend) if Arc::ptr_eq(backend, &admission.backend)
        )
}

fn audit_rotation_publish_failure(principal: &Principal, store: &str) {
    log_admin_event(
        "store.rotate-key",
        principal,
        AuditEntity::Store { store },
        Some("publish-failed"),
        "failure",
    );
}

async fn rotate_key_with_hooks(
    state: Arc<ListenerState>,
    principal: Principal,
    name: String,
    hooks: &dyn rotation::RotationHooks,
) -> Result<Json<AdminStoreRotateKeyResponse>, AdminError> {
    // Resolve only the stable entry-owned mutex before waiting. The same mutex
    // identity must still own the entry after admission and at publication.
    let rotation_lock = {
        let registry = state.shared.stores.read().unwrap();
        Arc::clone(
            &registry
                .entry(&name)
                .map_err(|_| AdminError::store_not_found())?
                .rotation_lock,
        )
    };
    hooks.before_rotation_lock();
    let _rotation_guard = rotation_lock.lock().await;

    let admission = {
        let registry = state.shared.stores.read().unwrap();
        let Ok(entry) = registry.entry(&name) else {
            audit_rotation_publish_failure(&principal, &name);
            return Err(AdminError::rotation_recovery_required());
        };
        if !Arc::ptr_eq(&entry.rotation_lock, &rotation_lock) {
            audit_rotation_publish_failure(&principal, &name);
            return Err(AdminError::rotation_recovery_required());
        }
        let canonical = match &entry.record.storage {
            underwrap_protocol::config::StorageConfig::Plain { .. } => {
                return Err(AdminError::conflict(
                    "not_encrypted",
                    "store is not encrypted; rotate-key only applies to encrypted backends",
                ));
            }
            underwrap_protocol::config::StorageConfig::Encrypted { dir } => dir.to_path_buf(),
        };
        let backend = match &entry.state {
            StoreState::Unlocked(backend) => Arc::clone(backend),
            StoreState::Locked => return Err(AdminError::store_locked(&name)),
        };
        RotationAdmission {
            rotation_lock: Arc::clone(&rotation_lock),
            canonical,
            backend,
        }
    };

    let result = rotation::rotate_generation(
        &name,
        &admission.canonical,
        admission.backend.as_ref(),
        hooks,
    );
    let rotated = match result {
        Ok(rotated) => rotated,
        Err(failure) => {
            if failure.disposition == rotation::FailureDisposition::Lock {
                let mut registry = state.shared.stores.write().unwrap();
                if matches!(
                    registry.entries.get(&name),
                    Some(entry) if entry_matches_admission(entry, &admission)
                ) {
                    registry.lock(&name);
                }
            }
            log_admin_event(
                "store.rotate-key",
                &principal,
                AuditEntity::Store { store: &name },
                Some(failure.audit_reason),
                "failure",
            );
            return Err(failure.error);
        }
    };

    hooks.before_publish();
    let published = {
        let mut registry = state.shared.stores.write().unwrap();
        match registry.entries.get_mut(&name) {
            Some(entry) if entry_matches_admission(entry, &admission) => {
                entry.state = StoreState::Unlocked(Arc::clone(&rotated.backend));
                true
            }
            _ => false,
        }
    };
    if !published {
        audit_rotation_publish_failure(&principal, &name);
        rotated.discard();
        return Err(AdminError::rotation_recovery_required());
    }

    log_admin_event(
        "store.rotate-key",
        &principal,
        AuditEntity::Store { store: &name },
        None,
        "success",
    );
    Ok(Json(AdminStoreRotateKeyResponse {
        key_hex: rotated.key_hex,
        warning: "every resource in this store has been re-encrypted under the new master key. the complete previous ciphertext generation remains on the backend host. it consumes disk and remains decryptable with the old key. record and independently verify this replacement key after restart/unlock and representative reads, then have a trusted host operator remove only the selected obsolete recovery generation. this browser acknowledgement is local only; the backend cannot know that the key reached external custody."
            .to_string(),
    }))
}

/// `POST /v1/admin/stores/{id}/unlock` — accept a 64-character hex
/// master key from the operator, build a fresh `EncryptedBackend`
/// against the store's data directory, verify the key by decrypting
/// the unlock canary written at creation, and install the backend in
/// the registry. Wrong keys never transition the registry to
/// `Unlocked`; the registry stays `Locked` and the response surfaces a
/// structured `key_mismatch` code.
#[utoipa::path(
    post,
    path = "/v1/admin/stores/{id}/unlock",
    tag = "stores",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    request_body = AdminStoreUnlockRequest,
    responses(
        (status = 200, description = "Store unlocked.", body = AdminOkResponse),
        (status = 400, description = "Wrong key / passphrase.", body = ServiceErrorResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 500, description = "Canonical encrypted generation is missing; trusted-host recovery is required.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_unlock(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
    Body(body): Body<AdminStoreUnlockRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let dir = {
        let reg = state.shared.stores.read().unwrap();
        let entry = reg
            .entry(&name)
            .map_err(|_| AdminError::store_not_found())?;
        let dir = match &entry.record.storage {
            underwrap_protocol::config::StorageConfig::Plain { .. } => {
                return Err(AdminError::bad_request(
                    "store_not_encrypted",
                    "plain stores are never locked",
                ));
            }
            underwrap_protocol::config::StorageConfig::Encrypted { dir } => dir.to_path_buf(),
        };
        match &entry.state {
            StoreState::Unlocked(_) => {
                return Err(AdminError::conflict(
                    "already_unlocked",
                    "store is already unlocked",
                ));
            }
            StoreState::Locked => {}
        }
        dir
    };
    let mut key_bytes = match underwrap_backend_store::parse_hex_key(&body.key_hex) {
        Ok(k) => k,
        Err(e) => {
            log_admin_event(
                "store.unlock",
                &principal,
                AuditEntity::Store { store: &name },
                Some("invalid-key-hex"),
                "failure",
            );
            return Err(AdminError::bad_request(
                "invalid_key_hex",
                format!("master key is not 64 hex characters: {e}"),
            ));
        }
    };
    if matches!(
        std::fs::symlink_metadata(&dir),
        Err(ref error) if error.kind() == std::io::ErrorKind::NotFound
    ) {
        key_bytes.fill(0);
        log_admin_event(
            "store.unlock",
            &principal,
            AuditEntity::Store { store: &name },
            Some("recovery-required"),
            "failure",
        );
        return Err(AdminError::rotation_recovery_required());
    }
    let backend_result =
        underwrap_backend_store::EncryptedBackend::open_existing_with_key(&dir, key_bytes);
    key_bytes.fill(0);
    let backend = match backend_result {
        Ok(backend) => backend,
        Err(_) => {
            log_admin_event(
                "store.unlock",
                &principal,
                AuditEntity::Store { store: &name },
                Some("open-failed"),
                "failure",
            );
            return Err(AdminError::internal_with_code(
                "open_store_failed",
                "opening the existing encrypted store failed",
            ));
        }
    };
    match backend.read(underwrap_backend_store::ENCRYPTED_CANARY_ID) {
        Ok(Some(plain))
            if plain.as_ref() == underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT => {}
        Ok(Some(_)) => {
            log_admin_event(
                "store.unlock",
                &principal,
                AuditEntity::Store { store: &name },
                Some("canary-mismatch"),
                "failure",
            );
            return Err(AdminError::forbidden(
                "key_mismatch",
                "submitted key decrypts the canary but to unexpected content; refusing to unlock",
            ));
        }
        Ok(None) => {
            log_admin_event(
                "store.unlock",
                &principal,
                AuditEntity::Store { store: &name },
                Some("canary-missing"),
                "failure",
            );
            return Err(AdminError::forbidden(
                "key_mismatch",
                "store has no unlock canary; refusing to unlock under an unverifiable key",
            ));
        }
        Err(_) => {
            log_admin_event(
                "store.unlock",
                &principal,
                AuditEntity::Store { store: &name },
                Some("key-mismatch"),
                "failure",
            );
            return Err(AdminError::forbidden(
                "key_mismatch",
                "submitted key did not decrypt the unlock canary",
            ));
        }
    }
    {
        let mut reg = state.shared.stores.write().unwrap();
        reg.unlock(&name, Arc::new(backend));
    }
    log_admin_event(
        "store.unlock",
        &principal,
        AuditEntity::Store { store: &name },
        None,
        "success",
    );
    Ok(Json(AdminOkResponse { ok: true }))
}

/// `POST /v1/admin/stores/{id}/lock` — drop the in-memory backend for
/// `name`, transitioning the registry to [`StoreState::Locked`]. The
/// held `MasterKey` zeroes on drop. Plain stores reject the
/// operation; already-locked stores succeed as a no-op so the admin
/// SPA can call this without racing the current state.
#[utoipa::path(
    post,
    path = "/v1/admin/stores/{id}/lock",
    tag = "stores",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    responses(
        (status = 200, description = "Store locked; in-memory key material wiped.", body = AdminStoreLockResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_lock(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<AdminStoreLockResponse>, AdminError> {
    let mut reg = state.shared.stores.write().unwrap();
    let entry = reg
        .entry(&name)
        .map_err(|_| AdminError::store_not_found())?;
    if matches!(
        entry.record.storage,
        underwrap_protocol::config::StorageConfig::Plain { .. }
    ) {
        return Err(AdminError::bad_request(
            "store_not_encrypted",
            "plain stores are never locked",
        ));
    }
    let was_locked = matches!(entry.state, StoreState::Locked);
    reg.lock(&name);
    drop(reg);
    let result = if was_locked { "noop" } else { "success" };
    log_admin_event(
        "store.lock",
        &principal,
        AuditEntity::Store { store: &name },
        None,
        result,
    );
    Ok(Json(AdminStoreLockResponse {
        ok: true,
        was_locked,
    }))
}

#[cfg(test)]
#[path = "lifecycle/audit_tests.rs"]
mod audit_tests;
#[cfg(test)]
#[path = "lifecycle/concurrency_tests.rs"]
mod concurrency_tests;
#[cfg(test)]
mod tests;
