//! Encrypted-store lifecycle: `unlock`, `lock`, `rotate-key`.

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

use underwrap_protocol::service::AdminStoreRotateKeyResponse;
use underwrap_store::StorageBackend;

use crate::http::responses::{
    error_response, json_response, parse_json_body, store_locked_response,
};
use crate::runtime_state::AdminRuntimeState;
use crate::store_registry::{StoreState, save_store_records};

use super::{
    create_data_dir_secure, generate_master_key_bytes, master_key_hex, write_admin_store_audit,
};

/// Get or lazily create the rotation `Mutex` for `store_id`. Held for
/// the duration of a rotation so concurrent writes wait until the new
/// backend has fully replaced the old one.
fn rotation_lock(state: &AdminRuntimeState, store_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = state.rotation_locks.lock().unwrap();
    Arc::clone(
        locks
            .entry(store_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    )
}

/// `POST /v1/admin/stores/{id}/rotate-key` — mint a fresh 32-byte
/// master key in memory, re-encrypt every resource in the store under
/// it, atomically swap the data directory, install the new backend in
/// the registry, and return the new key **exactly once**. Plain stores
/// reject the operation; locked encrypted stores must be unlocked
/// first.
pub(super) async fn handle_admin_store_rotate_key(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
) -> Response<Full<Bytes>> {
    let (mut updated_record, dir, current_backend) = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        let record = loaded.record.clone();
        let dir = match &record.storage {
            underwrap_protocol::config::StorageConfig::Plain { .. } => {
                return error_response(
                    StatusCode::CONFLICT,
                    "not_encrypted",
                    "store is not encrypted; rotate-key only applies to encrypted backends",
                );
            }
            underwrap_protocol::config::StorageConfig::Encrypted { dir } => dir.clone(),
        };
        match reg.state.get(name) {
            Some(StoreState::Unlocked(b)) => (record, dir, Arc::clone(b)),
            Some(StoreState::Locked) => {
                return store_locked_response(name);
            }
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "store metadata present but registry state missing",
                );
            }
        }
    };

    let rotate_lock = rotation_lock(state, name);
    let _rotate_guard = rotate_lock.lock().await;

    let new_key_bytes = generate_master_key_bytes();
    let key_hex = master_key_hex(&new_key_bytes);

    // Stage the re-encrypted blobs in a sibling temp directory and
    // swap into place once every resource has migrated. The temp dir
    // is colocated with the data dir so `rename` is atomic on the
    // same filesystem.
    let parent = match dir.parent() {
        Some(p) => p.to_path_buf(),
        None => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "encrypted store directory has no parent — cannot stage rotation",
            );
        }
    };
    let stage_name = format!(".rotating-{}-{}", name, crate::ids::random_hex_8());
    let stage_dir = parent.join(&stage_name);
    if let Err(e) = create_data_dir_secure(&stage_dir) {
        write_admin_store_audit(
            principal,
            name,
            "store.rotate-key",
            Some("stage-failed"),
            "failure",
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rotate_stage_failed",
            &format!(
                "creating rotation staging dir {}: {e:#}",
                stage_dir.display()
            ),
        );
    }
    let new_backend =
        match underwrap_store::EncryptedBackend::open_with_key(&stage_dir, new_key_bytes) {
            Ok(b) => b,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&stage_dir);
                write_admin_store_audit(
                    principal,
                    name,
                    "store.rotate-key",
                    Some("stage-failed"),
                    "failure",
                );
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "rotate_stage_failed",
                    &format!("opening rotation staging backend: {e:#}"),
                );
            }
        };

    if let Err(resp) = reencrypt_into(
        current_backend.as_ref(),
        &new_backend,
        &stage_dir,
        principal,
        name,
    ) {
        return resp;
    }

    // Move the live data dir aside and swap the staged dir in. Both
    // renames are individually atomic; we attempt to undo the first
    // if the second fails so the registry's `Arc` never points at a
    // backend whose directory has vanished.
    let backup_dir = parent.join(format!("{stage_name}.replaced"));
    if let Err(e) = std::fs::rename(&dir, &backup_dir) {
        let _ = std::fs::remove_dir_all(&stage_dir);
        write_admin_store_audit(
            principal,
            name,
            "store.rotate-key",
            Some("swap-failed"),
            "failure",
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rotate_swap_failed",
            &format!(
                "moving live data dir aside ({} → {}): {e:#}",
                dir.display(),
                backup_dir.display(),
            ),
        );
    }
    if let Err(e) = std::fs::rename(&stage_dir, &dir) {
        // Try to roll the first rename back so the live dir keeps
        // pointing at usable ciphertext.
        let _ = std::fs::rename(&backup_dir, &dir);
        let _ = std::fs::remove_dir_all(&stage_dir);
        write_admin_store_audit(
            principal,
            name,
            "store.rotate-key",
            Some("swap-failed"),
            "failure",
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rotate_swap_failed",
            &format!(
                "promoting staged data dir ({} → {}): {e:#}",
                stage_dir.display(),
                dir.display(),
            ),
        );
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
                write_admin_store_audit(
                    principal,
                    name,
                    "store.rotate-key",
                    Some("reopen-failed"),
                    "failure",
                );
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &format!("parsing fresh rotation key back from hex: {e:#}"),
                );
            }
        };
        match underwrap_store::EncryptedBackend::open_with_key(&dir, bytes) {
            Ok(b) => Arc::new(b),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&backup_dir);
                write_admin_store_audit(
                    principal,
                    name,
                    "store.rotate-key",
                    Some("reopen-failed"),
                    "failure",
                );
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "open_store_failed",
                    &format!("opening rotated backend at {}: {e:#}", dir.display()),
                );
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
        let mut reg = state.stores.write().unwrap();
        reg.state
            .insert(name.to_string(), StoreState::Unlocked(rotated_backend));
        if let Some(loaded) = reg.records.get_mut(name) {
            loaded.record = updated_record.clone();
        }
        if let Err(e) = save_store_records(&state.stores_file, &reg.records) {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "write_store_registry_failed",
                &format!("{e:#}"),
            );
        }
    }

    write_admin_store_audit(principal, name, "store.rotate-key", None, "success");
    json_response(
        StatusCode::OK,
        &AdminStoreRotateKeyResponse {
            key_hex,
            warning:
                "every resource in this store has been re-encrypted under the new master key. \
                 the new key is shown only here — record it now in a password manager or other \
                 trusted store. you will need it to unlock this store after the next backend \
                 restart."
                    .to_string(),
        },
    )
}

/// Copy every resource (and the canary) from `from` to `to`,
/// transparently decrypting under the old key and re-encrypting under
/// the new one. On any failure the staged data directory is removed
/// and a structured error response is returned for the rotate handler
/// to surface to the operator.
#[allow(clippy::result_large_err)]
fn reencrypt_into(
    from: &dyn StorageBackend,
    to: &dyn StorageBackend,
    stage_dir: &Path,
    principal: &crate::users::Principal,
    store_name: &str,
) -> std::result::Result<(), Response<Full<Bytes>>> {
    let ids = match from.list("") {
        Ok(ids) => ids,
        Err(e) => {
            let _ = std::fs::remove_dir_all(stage_dir);
            write_admin_store_audit(
                principal,
                store_name,
                "store.rotate-key",
                Some("list-failed"),
                "failure",
            );
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "rotate_list_failed",
                &format!("listing existing resources for rotation: {e:#}"),
            ));
        }
    };
    // Make sure the canary is always present in the new backend even
    // if the source store somehow lost it (e.g. operator deleted it
    // by hand). Write it once up front; the loop below will skip its
    // own re-write so we don't fight ourselves over the canary id.
    if let Err(e) = to.write(
        underwrap_store::ENCRYPTED_CANARY_ID,
        underwrap_store::ENCRYPTED_CANARY_PLAINTEXT,
    ) {
        let _ = std::fs::remove_dir_all(stage_dir);
        write_admin_store_audit(
            principal,
            store_name,
            "store.rotate-key",
            Some("canary-write-failed"),
            "failure",
        );
        return Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rotate_canary_failed",
            &format!("writing canary in staged data dir: {e:#}"),
        ));
    }
    for id in ids {
        if id == underwrap_store::ENCRYPTED_CANARY_ID {
            continue;
        }
        let material = match from.read(&id) {
            Ok(Some(m)) => m,
            Ok(None) => continue, // raced — gone
            Err(e) => {
                let _ = std::fs::remove_dir_all(stage_dir);
                write_admin_store_audit(
                    principal,
                    store_name,
                    "store.rotate-key",
                    Some("read-failed"),
                    "failure",
                );
                return Err(error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "rotate_read_failed",
                    &format!("reading '{id}' for rotation: {e:#}"),
                ));
            }
        };
        if let Err(e) = to.write(&id, material.as_ref()) {
            let _ = std::fs::remove_dir_all(stage_dir);
            write_admin_store_audit(
                principal,
                store_name,
                "store.rotate-key",
                Some("write-failed"),
                "failure",
            );
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "rotate_write_failed",
                &format!("writing '{id}' to staged data dir: {e:#}"),
            ));
        }
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
/// HTTP entry point. Split from [`handle_admin_store_unlock`] so unit
/// tests in `stores/lifecycle/tests.rs` can drive the unlock directly.
pub(super) async fn handle_admin_store_unlock_endpoint(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<underwrap_protocol::service::AdminStoreUnlockRequest>(
        request,
    )
    .await
    {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    handle_admin_store_unlock(state, principal, name, &body)
}

pub(crate) fn handle_admin_store_unlock(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    body: &underwrap_protocol::service::AdminStoreUnlockRequest,
) -> Response<Full<Bytes>> {
    let dir = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        match &loaded.record.storage {
            underwrap_protocol::config::StorageConfig::Plain { .. } => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "store_not_encrypted",
                    "plain stores are never locked",
                );
            }
            underwrap_protocol::config::StorageConfig::Encrypted { dir } => dir.clone(),
        }
    };
    match state.stores.read().unwrap().state.get(name) {
        Some(StoreState::Unlocked(_)) => {
            return error_response(
                StatusCode::CONFLICT,
                "already_unlocked",
                "store is already unlocked",
            );
        }
        Some(StoreState::Locked) => {}
        None => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "store metadata present but registry state missing",
            );
        }
    }
    let key_bytes = match underwrap_store::parse_hex_key(&body.key_hex) {
        Ok(k) => k,
        Err(e) => {
            write_admin_store_audit(
                principal,
                name,
                "store.unlock",
                Some("invalid-key-hex"),
                "failure",
            );
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_key_hex",
                &format!("master key is not 64 hex characters: {e}"),
            );
        }
    };
    let backend = match underwrap_store::EncryptedBackend::open_with_key(&dir, key_bytes) {
        Ok(b) => b,
        Err(e) => {
            write_admin_store_audit(
                principal,
                name,
                "store.unlock",
                Some("open-failed"),
                "failure",
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "open_store_failed",
                &format!("opening encrypted store at {}: {e:#}", dir.display()),
            );
        }
    };
    match backend.read(underwrap_store::ENCRYPTED_CANARY_ID) {
        Ok(Some(plain)) if plain.as_ref() == underwrap_store::ENCRYPTED_CANARY_PLAINTEXT => {}
        Ok(Some(_)) => {
            write_admin_store_audit(
                principal,
                name,
                "store.unlock",
                Some("canary-mismatch"),
                "failure",
            );
            return error_response(
                StatusCode::FORBIDDEN,
                "key_mismatch",
                "submitted key decrypts the canary but to unexpected content; refusing to unlock",
            );
        }
        Ok(None) => {
            write_admin_store_audit(
                principal,
                name,
                "store.unlock",
                Some("canary-missing"),
                "failure",
            );
            return error_response(
                StatusCode::FORBIDDEN,
                "key_mismatch",
                "store has no unlock canary; refusing to unlock under an unverifiable key",
            );
        }
        Err(_) => {
            write_admin_store_audit(
                principal,
                name,
                "store.unlock",
                Some("key-mismatch"),
                "failure",
            );
            return error_response(
                StatusCode::FORBIDDEN,
                "key_mismatch",
                "submitted key did not decrypt the unlock canary",
            );
        }
    }
    {
        let mut reg = state.stores.write().unwrap();
        reg.unlock(name, Arc::new(backend));
    }
    write_admin_store_audit(principal, name, "store.unlock", None, "success");
    json_response(StatusCode::OK, &serde_json::json!({ "ok": true }))
}

/// `POST /v1/admin/stores/{id}/lock` — drop the in-memory backend for
/// `name`, transitioning the registry to [`StoreState::Locked`]. The
/// held `MasterKey` zeroes on drop. Plain stores reject the
/// operation; already-locked stores succeed as a no-op so the admin
/// SPA can call this without racing the current state.
pub(super) fn handle_admin_store_lock(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
) -> Response<Full<Bytes>> {
    let mut reg = state.stores.write().unwrap();
    let Some(loaded) = reg.records.get(name) else {
        return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
    };
    if matches!(
        loaded.record.storage,
        underwrap_protocol::config::StorageConfig::Plain { .. }
    ) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "store_not_encrypted",
            "plain stores are never locked",
        );
    }
    let was_locked = matches!(reg.state.get(name), Some(StoreState::Locked));
    reg.lock(name);
    drop(reg);
    let result = if was_locked { "noop" } else { "success" };
    write_admin_store_audit(principal, name, "store.lock", None, result);
    json_response(
        StatusCode::OK,
        &serde_json::json!({ "ok": true, "was_locked": was_locked }),
    )
}

#[cfg(test)]
mod tests;
