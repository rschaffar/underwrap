//! Encrypted-store lifecycle: `unlock`, `lock`, `rotate-key`.

use std::path::Path;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxumPath, State};
use tracing::warn;
use underwrap_protocol::service::{
    AdminOkResponse, AdminStoreLockResponse, AdminStoreRotateKeyResponse, AdminStoreUnlockRequest,
    ServiceErrorResponse,
};
use underwrap_store::StorageBackend;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::csrf::Principal;
use crate::http::responses::Body;
use crate::runtime_state::ListenerState;
use crate::store_registry::{StoreState, save_store_record};

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<ListenerState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_store_rotate_key))
        .routes(routes!(handle_admin_store_unlock))
        .routes(routes!(handle_admin_store_lock))
}

use super::{create_data_dir_secure, generate_master_key_bytes, master_key_hex};
use crate::admin_api::audit::{AuditEntity, log_admin_event};

/// `POST /v1/admin/stores/{id}/rotate-key` — mint a fresh 32-byte
/// master key in memory, re-encrypt every resource in the store under
/// it, atomically swap the data directory, install the new backend in
/// the registry, and return the new key **exactly once**. Plain stores
/// reject the operation; locked encrypted stores must be unlocked
/// first.
#[utoipa::path(
    post,
    path = "/v1/admin/stores/{id}/rotate-key",
    tag = "stores",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    responses(
        (status = 200, description = "Key rotated; old material persisted alongside the new one.", body = AdminStoreRotateKeyResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
        (status = 500, description = "Rotation failed mid-flight.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_rotate_key(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<AdminStoreRotateKeyResponse>, AdminError> {
    let (mut updated_record, dir, current_backend, rotate_lock) = {
        let reg = state.shared.stores.read().unwrap();
        let entry = reg
            .entry(&name)
            .map_err(|_| AdminError::store_not_found())?;
        let record = entry.record.clone();
        let dir = match &record.storage {
            underwrap_protocol::config::StorageConfig::Plain { .. } => {
                return Err(AdminError::conflict(
                    "not_encrypted",
                    "store is not encrypted; rotate-key only applies to encrypted backends",
                ));
            }
            underwrap_protocol::config::StorageConfig::Encrypted { dir } => dir.to_path_buf(),
        };
        let rotate_lock = Arc::clone(&entry.rotation_lock);
        match &entry.state {
            StoreState::Unlocked(b) => (record, dir, Arc::clone(b), rotate_lock),
            StoreState::Locked => return Err(AdminError::store_locked(&name)),
        }
    };

    let _rotate_guard = rotate_lock.lock().await;

    let new_key_bytes = generate_master_key_bytes();
    let key_hex = master_key_hex(&new_key_bytes);

    // Stage the re-encrypted blobs in a sibling temp directory and
    // swap into place once every resource has migrated. The temp dir
    // is colocated with the data dir so `rename` is atomic on the
    // same filesystem.
    let parent = dir
        .parent()
        .ok_or_else(|| {
            AdminError::internal_with_code(
                "internal",
                "encrypted store directory has no parent — cannot stage rotation",
            )
        })?
        .to_path_buf();
    let stage_name = format!(".rotating-{}-{}", name, crate::ids::random_hex_8());
    let stage_dir = parent.join(&stage_name);
    if let Err(e) = create_data_dir_secure(&stage_dir) {
        log_admin_event(
            "store.rotate-key",
            &principal,
            AuditEntity::Store { store: &name },
            Some("stage-failed"),
            "failure",
        );
        return Err(AdminError::internal_with_code(
            "rotate_stage_failed",
            format!(
                "creating rotation staging dir {}: {e:#}",
                stage_dir.display()
            ),
        ));
    }
    let new_backend =
        match underwrap_store::EncryptedBackend::open_with_key(&stage_dir, new_key_bytes) {
            Ok(b) => b,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&stage_dir);
                log_admin_event(
                    "store.rotate-key",
                    &principal,
                    AuditEntity::Store { store: &name },
                    Some("stage-failed"),
                    "failure",
                );
                return Err(AdminError::internal_with_code(
                    "rotate_stage_failed",
                    format!("opening rotation staging backend: {e:#}"),
                ));
            }
        };

    reencrypt_into(
        &*current_backend,
        &new_backend,
        &stage_dir,
        &principal,
        &name,
    )?;

    // Move the live data dir aside and swap the staged dir in. Both
    // renames are individually atomic; we attempt to undo the first
    // if the second fails so the registry's `Arc` never points at a
    // backend whose directory has vanished.
    let backup_dir = parent.join(format!("{stage_name}.replaced"));
    if let Err(e) = std::fs::rename(&dir, &backup_dir) {
        let _ = std::fs::remove_dir_all(&stage_dir);
        log_admin_event(
            "store.rotate-key",
            &principal,
            AuditEntity::Store { store: &name },
            Some("swap-failed"),
            "failure",
        );
        return Err(AdminError::internal_with_code(
            "rotate_swap_failed",
            format!(
                "moving live data dir aside ({} → {}): {e:#}",
                dir.display(),
                backup_dir.display(),
            ),
        ));
    }
    if let Err(e) = std::fs::rename(&stage_dir, &dir) {
        // Try to roll the first rename back so the live dir keeps
        // pointing at usable ciphertext.
        let _ = std::fs::rename(&backup_dir, &dir);
        let _ = std::fs::remove_dir_all(&stage_dir);
        log_admin_event(
            "store.rotate-key",
            &principal,
            AuditEntity::Store { store: &name },
            Some("swap-failed"),
            "failure",
        );
        return Err(AdminError::internal_with_code(
            "rotate_swap_failed",
            format!(
                "promoting staged data dir ({} → {}): {e:#}",
                stage_dir.display(),
                dir.display(),
            ),
        ));
    }

    // Open a fresh backend at the post-swap path so the registry
    // ends up holding an `Arc` whose `dir` field matches the live
    // directory. (Reusing `new_backend` would carry the staging
    // path even though we just renamed it.)
    let rotated_backend: Arc<dyn StorageBackend> = {
        // Re-derive the in-memory key from the hex we already
        // computed — that way we don't have to hold on to two
        // copies of the byte form across the swap.
        let bytes = match underwrap_store::parse_hex_key(&key_hex) {
            Ok(b) => b,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&backup_dir);
                log_admin_event(
                    "store.rotate-key",
                    &principal,
                    AuditEntity::Store { store: &name },
                    Some("reopen-failed"),
                    "failure",
                );
                return Err(AdminError::internal(
                    "parsing fresh rotation key back from hex",
                    e,
                ));
            }
        };
        match underwrap_store::EncryptedBackend::open_with_key(&dir, bytes) {
            Ok(b) => Arc::new(b),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&backup_dir);
                log_admin_event(
                    "store.rotate-key",
                    &principal,
                    AuditEntity::Store { store: &name },
                    Some("reopen-failed"),
                    "failure",
                );
                return Err(AdminError::internal_with_code(
                    "open_store_failed",
                    format!("opening rotated backend at {}: {e:#}", dir.display()),
                ));
            }
        }
    };
    // Best-effort cleanup of the old ciphertext directory now that
    // the swap has succeeded.
    if let Err(e) = std::fs::remove_dir_all(&backup_dir) {
        warn!(
            store = %name,
            backup_dir = %backup_dir.display(),
            err = format!("{e:#}"),
            "rotation succeeded but old data dir could not be removed; left in place for manual cleanup",
        );
    }

    updated_record.storage = underwrap_protocol::config::StorageConfig::Encrypted { dir };
    {
        let mut reg = state.shared.stores.write().unwrap();
        if let Some(entry) = reg.entries.get_mut(&name) {
            entry.state = StoreState::Unlocked(rotated_backend);
            entry.record = updated_record.clone();
        }
        if let Err(e) = save_store_record(&state.admin().stores_dir, &updated_record) {
            return Err(AdminError::internal_with_code(
                "write_store_registry_failed",
                format!("{e:#}"),
            ));
        }
    }

    log_admin_event(
        "store.rotate-key",
        &principal,
        AuditEntity::Store { store: &name },
        None,
        "success",
    );
    Ok(Json(AdminStoreRotateKeyResponse {
        key_hex,
        warning: "every resource in this store has been re-encrypted under the new master key. \
             the new key is shown only here — record it now in a password manager or other \
             trusted store. you will need it to unlock this store after the next backend \
             restart."
            .to_string(),
    }))
}

/// Copy every resource (and the canary) from `from` to `to`,
/// transparently decrypting under the old key and re-encrypting under
/// the new one. On any failure the staged data directory is removed
/// and a structured `AdminError` is returned for the rotate handler
/// to surface to the operator.
fn reencrypt_into(
    from: &dyn StorageBackend,
    to: &dyn StorageBackend,
    stage_dir: &Path,
    principal: &Principal,
    store_name: &str,
) -> Result<(), AdminError> {
    let bail = |reason: &'static str, code: &str, message: String| -> AdminError {
        let _ = std::fs::remove_dir_all(stage_dir);
        log_admin_event(
            "store.rotate-key",
            principal,
            AuditEntity::Store { store: store_name },
            Some(reason),
            "failure",
        );
        AdminError::internal_with_code(code.to_string(), message)
    };
    let ids = from.list("").map_err(|e| {
        bail(
            "list-failed",
            "rotate_list_failed",
            format!("listing existing resources for rotation: {e:#}"),
        )
    })?;
    // Make sure the canary is always present in the new backend even
    // if the source store somehow lost it (e.g. operator deleted it
    // by hand). Write it once up front; the loop below will skip its
    // own re-write so we don't fight ourselves over the canary id.
    to.write(
        underwrap_store::ENCRYPTED_CANARY_ID,
        underwrap_store::ENCRYPTED_CANARY_PLAINTEXT,
    )
    .map_err(|e| {
        bail(
            "canary-write-failed",
            "rotate_canary_failed",
            format!("writing canary in staged data dir: {e:#}"),
        )
    })?;
    for id in ids {
        if id == underwrap_store::ENCRYPTED_CANARY_ID {
            continue;
        }
        let material = match from.read(&id).map_err(|e| {
            bail(
                "read-failed",
                "rotate_read_failed",
                format!("reading '{id}' for rotation: {e:#}"),
            )
        })? {
            Some(m) => m,
            None => continue, // raced — gone
        };
        to.write(&id, material.as_ref()).map_err(|e| {
            bail(
                "write-failed",
                "rotate_write_failed",
                format!("writing '{id}' to staged data dir: {e:#}"),
            )
        })?;
    }
    Ok(())
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
    let key_bytes = match underwrap_store::parse_hex_key(&body.key_hex) {
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
    let backend = match underwrap_store::EncryptedBackend::open_with_key(&dir, key_bytes) {
        Ok(b) => b,
        Err(e) => {
            log_admin_event(
                "store.unlock",
                &principal,
                AuditEntity::Store { store: &name },
                Some("open-failed"),
                "failure",
            );
            return Err(AdminError::internal_with_code(
                "open_store_failed",
                format!("opening encrypted store at {}: {e:#}", dir.display()),
            ));
        }
    };
    match backend.read(underwrap_store::ENCRYPTED_CANARY_ID) {
        Ok(Some(plain)) if plain.as_ref() == underwrap_store::ENCRYPTED_CANARY_PLAINTEXT => {}
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
mod tests;
