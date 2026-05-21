//! Store admin CRUD + lifecycle: `/v1/admin/stores/*`.
//!
//! Covers `list`, `detail`, `create`, `update`, `delete` plus the
//! three lifecycle operations `unlock`, `lock`, and `rotate-key`. They
//! share `admin_store`, the registry helpers, and the rotation
//! `Mutex`, so they live together as one subdomain rather than split
//! per-verb.
//!
//! All five CRUD handlers operate on the runtime `state.stores`
//! registry plus the backend-managed store-registry file at
//! `state.stores_file`. Atomicity comes from `tempfile::NamedTempFile`
//! `persist()` on the registry write side and a brief held `write()`
//! lock on the in-memory registry update.

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::warn;

use underwrap_protocol::config::StoreRecord;
use underwrap_protocol::service::{
    AdminStoreActionState, AdminStoreBackendKind, AdminStoreCreateRequest,
    AdminStoreCreateResponse, AdminStoreDeleteResponse, AdminStoreDetailResponse,
    AdminStoreListItem, AdminStoreRotateKeyResponse, AdminStoreUpdateRequest,
    AdminStoreUsageSummary, AdminStoresListResponse,
};
use underwrap_store::StorageBackend;

use crate::admin_api::audit::log_admin_event;
use crate::http::responses::{
    error_response, json_response, parse_json_body, store_locked_response,
};
use crate::runtime_state::AdminRuntimeState;
use crate::store_registry::{
    LoadedStoreRecord, RegistryLookupError, StoreState, generate_store_id, save_store_records,
    validate_store_name,
};

/// Resolve a store reference for an authenticated request. Any
/// authenticated user can address any store; the helper just returns
/// the live backend or the appropriate not-found / locked response.
#[allow(clippy::result_large_err)]
pub(crate) fn admin_store(
    state: &AdminRuntimeState,
    store: &str,
) -> std::result::Result<Arc<dyn StorageBackend>, Response<Full<Bytes>>> {
    match state.stores.read().unwrap().unlocked_backend(store) {
        Ok(s) => Ok(s),
        Err(RegistryLookupError::Locked) => Err(store_locked_response(store)),
        Err(RegistryLookupError::NotFound) => Err(error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown store",
        )),
    }
}

fn backend_kind(storage: &underwrap_protocol::config::StorageConfig) -> AdminStoreBackendKind {
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

#[allow(clippy::result_large_err)]
fn admin_store_usage(
    state: &AdminRuntimeState,
    store_id: &str,
    store: &dyn underwrap_store::StorageBackend,
) -> std::result::Result<AdminStoreUsageSummary, Response<Full<Bytes>>> {
    let mapped_client_instance_count = saturating_u32(
        state
            .client_instances
            .read()
            .unwrap()
            .values()
            .filter(|instance| instance.current_store_id == store_id)
            .count(),
    );
    let has_secrets = match store.list("") {
        Ok(ids) => ids
            .iter()
            .any(|id| id != underwrap_store::ENCRYPTED_CANARY_ID),
        Err(e) => {
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "list_store_failed",
                &format!("checking store contents for management view: {e:#}"),
            ));
        }
    };
    Ok(AdminStoreUsageSummary {
        mapped_client_instance_count,
        has_secrets,
    })
}

fn admin_store_actions(
    loaded: &LoadedStoreRecord,
    usage: &AdminStoreUsageSummary,
    locked: bool,
) -> AdminStoreActionState {
    let can_manage = true;
    let is_encrypted = matches!(
        &loaded.record.storage,
        underwrap_protocol::config::StorageConfig::Encrypted { .. }
    );
    // Delete works for both locked and unlocked stores — we use the
    // `has_secrets` flag only to choose between plain DELETE and
    // force-DELETE. For locked stores we cannot inspect contents, so
    // we conservatively offer force-DELETE only.
    let can_delete_without_force =
        can_manage && usage.mapped_client_instance_count == 0 && !usage.has_secrets && !locked;
    let can_force_delete =
        can_manage && usage.mapped_client_instance_count == 0 && (usage.has_secrets || locked);
    AdminStoreActionState {
        can_edit_metadata: can_manage,
        can_rotate_key: can_manage && is_encrypted && !locked,
        can_unlock: can_manage && is_encrypted && locked,
        can_lock: can_manage && is_encrypted && !locked,
        can_delete_now: can_delete_without_force,
        can_force_delete,
    }
}

fn admin_store_list_item(
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

fn admin_store_detail(
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
fn admin_store_usage_for_locked(
    state: &AdminRuntimeState,
    store_id: &str,
) -> AdminStoreUsageSummary {
    let mapped_client_instance_count = saturating_u32(
        state
            .client_instances
            .read()
            .unwrap()
            .values()
            .filter(|instance| instance.current_store_id == store_id)
            .count(),
    );
    AdminStoreUsageSummary {
        mapped_client_instance_count,
        has_secrets: false,
    }
}

pub(crate) fn handle_admin_stores_list(state: &AdminRuntimeState) -> Response<Full<Bytes>> {
    type Visible = (String, LoadedStoreRecord, Option<Arc<dyn StorageBackend>>);
    let visible: Vec<Visible> = {
        let reg = state.stores.read().unwrap();
        reg.records
            .iter()
            .map(|(id, loaded)| {
                let backend = reg.state.get(id).and_then(|s| s.backend()).cloned();
                (id.clone(), loaded.clone(), backend)
            })
            .collect()
    };
    let mut items = Vec::with_capacity(visible.len());
    for (id, loaded, handle) in visible {
        let (usage, locked) = match handle {
            Some(b) => match admin_store_usage(state, &id, b.as_ref()) {
                Ok(usage) => (usage, false),
                Err(resp) => return resp,
            },
            None => (admin_store_usage_for_locked(state, &id), true),
        };
        let actions = admin_store_actions(&loaded, &usage, locked);
        items.push(admin_store_list_item(&loaded, locked, usage, actions));
    }
    items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    json_response(StatusCode::OK, &AdminStoresListResponse { items })
}

#[allow(clippy::result_large_err)]
pub(crate) fn handle_admin_store_detail(
    state: &AdminRuntimeState,
    name: &str,
) -> Response<Full<Bytes>> {
    let (loaded, handle, locked) = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        match reg.state.get(name) {
            Some(StoreState::Unlocked(b)) => (loaded.clone(), Some(Arc::clone(b)), false),
            Some(StoreState::Locked) => (loaded.clone(), None, true),
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "store metadata present but registry state missing",
                );
            }
        }
    };
    let usage = match handle.as_ref() {
        Some(b) => match admin_store_usage(state, name, b.as_ref()) {
            Ok(usage) => usage,
            Err(resp) => return resp,
        },
        None => admin_store_usage_for_locked(state, name),
    };
    let actions = admin_store_actions(&loaded, &usage, locked);
    json_response(
        StatusCode::OK,
        &admin_store_detail(&state.stores_file, &loaded, locked, usage, actions),
    )
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

#[allow(clippy::result_large_err)]
pub(crate) async fn handle_admin_store_create(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminStoreCreateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_store_create(state, principal, body)
}

/// Body-already-parsed half of [`handle_admin_store_create`]. Split
/// out so unit tests can drive it without synthesising a
/// `Request<Incoming>` (which `hyper` doesn't expose constructors
/// for). The HTTP-shaped handler above is exercised by Playwright.
#[allow(clippy::result_large_err)]
pub(crate) fn do_admin_store_create(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: AdminStoreCreateRequest,
) -> Response<Full<Bytes>> {
    let visible_name = match validate_store_name(&body.name) {
        Ok(name) => name,
        Err(msg) => return error_response(StatusCode::BAD_REQUEST, "invalid_store_name", msg),
    };

    let store_id = loop {
        let candidate = generate_store_id();
        if !state
            .stores
            .read()
            .unwrap()
            .records
            .contains_key(&candidate)
        {
            break candidate;
        }
    };

    let validated_dir = match resolve_store_dir(state, &store_id) {
        Ok(p) => p,
        Err((code, msg)) => return error_response(StatusCode::BAD_REQUEST, code, &msg),
    };

    {
        let reg = state.stores.read().unwrap();
        if reg
            .records
            .values()
            .any(|existing| existing.record.name.eq_ignore_ascii_case(&visible_name))
        {
            return error_response(
                StatusCode::CONFLICT,
                "store_name_already_exists",
                &format!("store name '{}' already exists", visible_name),
            );
        }
    }

    let (storage, opened, generated_key_hex): (
        underwrap_protocol::config::StorageConfig,
        Arc<dyn StorageBackend>,
        Option<String>,
    ) = match body.backend {
        AdminStoreBackendKind::Plain => {
            let storage = underwrap_protocol::config::StorageConfig::Plain {
                dir: validated_dir.clone(),
            };
            if let Err(e) = create_data_dir_secure(&validated_dir) {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "create_data_dir_failed",
                    &format!("creating data dir {}: {e:#}", validated_dir.display()),
                );
            }
            let backend: Arc<dyn StorageBackend> = match underwrap_store::open(&storage) {
                Ok(b) => Arc::from(b),
                Err(e) => {
                    return error_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "open_store_failed",
                        &format!("opening store backend after create: {e:#}"),
                    );
                }
            };
            (storage, backend, None)
        }
        AdminStoreBackendKind::Encrypted => {
            if let Err(e) = create_data_dir_secure(&validated_dir) {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "create_data_dir_failed",
                    &format!("creating data dir {}: {e:#}", validated_dir.display()),
                );
            }
            let key_bytes = generate_master_key_bytes();
            let key_hex = master_key_hex(&key_bytes);
            let backend =
                match underwrap_store::EncryptedBackend::open_with_key(&validated_dir, key_bytes) {
                    Ok(b) => b,
                    Err(e) => {
                        return error_response(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "open_store_failed",
                            &format!("opening encrypted store backend after create: {e:#}"),
                        );
                    }
                };
            if let Err(e) = backend.write(
                underwrap_store::ENCRYPTED_CANARY_ID,
                underwrap_store::ENCRYPTED_CANARY_PLAINTEXT,
            ) {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "canary_write_failed",
                    &format!("writing unlock canary for new encrypted store: {e:#}"),
                );
            }
            let backend: Arc<dyn StorageBackend> = Arc::new(backend);
            let storage = underwrap_protocol::config::StorageConfig::Encrypted {
                dir: validated_dir.clone(),
            };
            (storage, backend, Some(key_hex))
        }
    };

    let record = StoreRecord {
        id: store_id.clone(),
        name: visible_name,
        description: body.description.clone(),
        created_at: Some(crate::users::now_epoch_secs()),
        storage,
    };

    let loaded = LoadedStoreRecord { record };
    {
        let mut reg = state.stores.write().unwrap();
        reg.state
            .insert(store_id.clone(), StoreState::Unlocked(opened));
        reg.records.insert(store_id.clone(), loaded.clone());
        if let Err(e) = save_store_records(&state.stores_file, &reg.records) {
            reg.state.remove(&store_id);
            reg.records.remove(&store_id);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "write_store_registry_failed",
                &format!("{e:#}"),
            );
        }
    }
    write_admin_store_audit(state, principal, &store_id, "store.create", "success");
    let usage = AdminStoreUsageSummary {
        mapped_client_instance_count: 0,
        has_secrets: false,
    };
    let actions = admin_store_actions(&loaded, &usage, false);
    json_response(
        StatusCode::OK,
        &AdminStoreCreateResponse {
            store: admin_store_detail(&state.stores_file, &loaded, false, usage, actions),
            key_hex: generated_key_hex,
        },
    )
}

/// Generate 32 fresh random bytes via the OS RNG. The bytes are
/// caller-owned and should be moved into either the [`EncryptedBackend`]
/// (which zeroizes on drop) or the JSON response buffer — never both.
fn generate_master_key_bytes() -> [u8; 32] {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes
}

/// Hex-encode a 32-byte master key as 64 lowercase characters.
fn master_key_hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(unix)]
fn create_data_dir_secure(dir: &Path) -> Result<()> {
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
fn create_data_dir_secure(dir: &Path) -> Result<()> {
    if dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))
}

#[allow(clippy::result_large_err)]
pub(crate) async fn handle_admin_store_update(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminStoreUpdateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_store_update(state, principal, name, body)
}

#[allow(clippy::result_large_err)]
pub(crate) fn do_admin_store_update(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    body: AdminStoreUpdateRequest,
) -> Response<Full<Bytes>> {
    let updated = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        let mut record = loaded.record.clone();
        if let Some(new_name) = body.name {
            let new_name = match validate_store_name(&new_name) {
                Ok(name) => name,
                Err(msg) => {
                    return error_response(StatusCode::BAD_REQUEST, "invalid_store_name", msg);
                }
            };
            if reg.records.iter().any(|(other_id, other)| {
                other_id != name && other.record.name.eq_ignore_ascii_case(&new_name)
            }) {
                return error_response(
                    StatusCode::CONFLICT,
                    "store_name_already_exists",
                    &format!("store name '{}' already exists", new_name),
                );
            }
            record.name = new_name;
        }
        if let Some(new_desc) = body.description {
            record.description = if new_desc.is_empty() {
                None
            } else {
                Some(new_desc)
            };
        }
        record
    };
    {
        let mut reg = state.stores.write().unwrap();
        if let Some(loaded) = reg.records.get_mut(name) {
            loaded.record = updated.clone();
        }
        if let Err(e) = save_store_records(&state.stores_file, &reg.records) {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "write_store_registry_failed",
                &format!("{e:#}"),
            );
        }
    }
    write_admin_store_audit(state, principal, name, "store.update", "success");
    let loaded = LoadedStoreRecord { record: updated };
    let (handle, locked) = {
        let reg = state.stores.read().unwrap();
        match reg.state.get(name) {
            Some(StoreState::Unlocked(b)) => (Some(Arc::clone(b)), false),
            Some(StoreState::Locked) => (None, true),
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "store metadata present but registry state missing",
                );
            }
        }
    };
    let usage = match handle.as_ref() {
        Some(b) => match admin_store_usage(state, name, b.as_ref()) {
            Ok(usage) => usage,
            Err(resp) => return resp,
        },
        None => admin_store_usage_for_locked(state, name),
    };
    let actions = admin_store_actions(&loaded, &usage, locked);
    json_response(
        StatusCode::OK,
        &admin_store_detail(&state.stores_file, &loaded, locked, usage, actions),
    )
}

#[allow(clippy::result_large_err)]
pub(crate) fn handle_admin_store_delete(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    force: bool,
) -> Response<Full<Bytes>> {
    let (store_handle, dir_for_hint, locked) = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        let dir = loaded.record.storage.dir().clone();
        match reg.state.get(name) {
            Some(StoreState::Unlocked(b)) => (Some(Arc::clone(b)), dir, false),
            Some(StoreState::Locked) => (None, dir, true),
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "store metadata present but registry state missing",
                );
            }
        }
    };
    if state
        .client_instances
        .read()
        .unwrap()
        .values()
        .any(|instance| instance.current_store_id == name)
    {
        return error_response(
            StatusCode::CONFLICT,
            "store_in_use",
            "store is still mapped to one or more client instances",
        );
    }

    let non_empty = match store_handle.as_ref() {
        Some(h) => match h.list("") {
            Ok(ids) => ids
                .iter()
                .any(|id| id != underwrap_store::ENCRYPTED_CANARY_ID),
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "list_store_failed",
                    &format!("checking store contents before delete: {e:#}"),
                );
            }
        },
        // Locked encrypted store: we cannot inspect contents. Refuse
        // unless the caller passed `force=true`, since deleting a
        // store without confirming it's empty risks orphaning data
        // the operator forgot about.
        None => true,
    };
    if non_empty && !force {
        let code = if locked {
            "store_locked_use_force"
        } else {
            "store_not_empty"
        };
        let message = if locked {
            "store is locked; contents cannot be inspected. re-issue with `?force=true` to delete the record anyway"
        } else {
            "store contains secrets; re-issue with `?force=true` to delete the record anyway"
        };
        return error_response(StatusCode::CONFLICT, code, message);
    }

    {
        let mut reg = state.stores.write().unwrap();
        reg.state.remove(name);
        reg.records.remove(name);
        if let Err(e) = save_store_records(&state.stores_file, &reg.records) {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "write_store_registry_failed",
                &format!("{e:#}"),
            );
        }
    }
    let cleanup_hint = if non_empty {
        Some(format!(
            "store '{name}' had secrets at deletion time. The store record has been removed; \
             the data dir at {} was left in place so you can decide whether to back it up first. \
             Remove it manually (`rm -rf {}`) when ready.",
            dir_for_hint.display(),
            dir_for_hint.display(),
        ))
    } else {
        None
    };
    let result = if non_empty {
        "force-deleted"
    } else {
        "success"
    };
    write_admin_store_audit(state, principal, name, "store.delete", result);
    json_response(
        StatusCode::OK,
        &AdminStoreDeleteResponse {
            ok: true,
            cleanup_hint,
        },
    )
}

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
pub(crate) async fn handle_admin_store_rotate_key(
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
    let stage_name = format!(
        ".rotating-{}-{}",
        name,
        master_key_hex(&generate_master_key_bytes())
            .chars()
            .take(16)
            .collect::<String>()
    );
    let stage_dir = parent.join(&stage_name);
    if let Err(e) = create_data_dir_secure(&stage_dir) {
        write_admin_store_audit(state, principal, name, "store.rotate-key", "stage-failed");
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
                write_admin_store_audit(state, principal, name, "store.rotate-key", "stage-failed");
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
        state,
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
        write_admin_store_audit(state, principal, name, "store.rotate-key", "swap-failed");
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
        write_admin_store_audit(state, principal, name, "store.rotate-key", "swap-failed");
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
    let installed_key = generate_master_key_bytes(); // placeholder, immediately overwritten
    let _ = installed_key; // keep clippy happy if it ever sees an unused let
    let rotated_backend: Arc<dyn StorageBackend> = {
        // Re-derive the in-memory key from the hex we already
        // computed — that way we don't have to hold on to two
        // copies of the byte form across the swap.
        let bytes = match underwrap_store::parse_hex_key(&key_hex) {
            Ok(b) => b,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&backup_dir);
                write_admin_store_audit(
                    state,
                    principal,
                    name,
                    "store.rotate-key",
                    "reopen-failed",
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
                    state,
                    principal,
                    name,
                    "store.rotate-key",
                    "reopen-failed",
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

    write_admin_store_audit(state, principal, name, "store.rotate-key", "success");
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
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    store_name: &str,
) -> std::result::Result<(), Response<Full<Bytes>>> {
    let ids = match from.list("") {
        Ok(ids) => ids,
        Err(e) => {
            let _ = std::fs::remove_dir_all(stage_dir);
            write_admin_store_audit(
                state,
                principal,
                store_name,
                "store.rotate-key",
                "list-failed",
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
            state,
            principal,
            store_name,
            "store.rotate-key",
            "canary-write-failed",
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
                    state,
                    principal,
                    store_name,
                    "store.rotate-key",
                    "read-failed",
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
                state,
                principal,
                store_name,
                "store.rotate-key",
                "write-failed",
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
pub(crate) async fn handle_admin_store_unlock(
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
    do_admin_store_unlock(state, principal, name, body)
}

#[allow(clippy::result_large_err)]
pub(crate) fn do_admin_store_unlock(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    body: underwrap_protocol::service::AdminStoreUnlockRequest,
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
            write_admin_store_audit(state, principal, name, "store.unlock", "invalid-key-hex");
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
            write_admin_store_audit(state, principal, name, "store.unlock", "open-failed");
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
            write_admin_store_audit(state, principal, name, "store.unlock", "canary-mismatch");
            return error_response(
                StatusCode::FORBIDDEN,
                "key_mismatch",
                "submitted key decrypts the canary but to unexpected content; refusing to unlock",
            );
        }
        Ok(None) => {
            write_admin_store_audit(state, principal, name, "store.unlock", "canary-missing");
            return error_response(
                StatusCode::FORBIDDEN,
                "key_mismatch",
                "store has no unlock canary; refusing to unlock under an unverifiable key",
            );
        }
        Err(_) => {
            write_admin_store_audit(state, principal, name, "store.unlock", "key-mismatch");
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
    write_admin_store_audit(state, principal, name, "store.unlock", "success");
    json_response(StatusCode::OK, &serde_json::json!({ "ok": true }))
}

/// `POST /v1/admin/stores/{id}/lock` — drop the in-memory backend for
/// `name`, transitioning the registry to [`StoreState::Locked`]. The
/// held `MasterKey` zeroes on drop. Plain stores reject the
/// operation; already-locked stores succeed as a no-op so the admin
/// SPA can call this without racing the current state.
#[allow(clippy::result_large_err)]
pub(crate) fn handle_admin_store_lock(
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
    write_admin_store_audit(state, principal, name, "store.lock", result);
    json_response(
        StatusCode::OK,
        &serde_json::json!({ "ok": true, "was_locked": was_locked }),
    )
}

pub(crate) fn write_admin_store_audit(
    _state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    store_name: &str,
    action: &str,
    result: &str,
) {
    let resource = format!("store:{store_name}");
    log_admin_event(
        action,
        &principal.username,
        None,
        Some(resource.as_str()),
        None,
        None,
        result,
    );
}

pub(crate) async fn route(
    method: &Method,
    path: &str,
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    if path == "/v1/admin/stores" {
        return match *method {
            Method::GET => handle_admin_stores_list(state),
            Method::POST => handle_admin_store_create(state, principal, request).await,
            _ => error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "GET or POST",
            ),
        };
    }
    if let Some(rest) = path.strip_prefix("/v1/admin/stores/") {
        if let Some(name) = rest.strip_suffix("/rotate-key")
            && !name.is_empty()
            && !name.contains('/')
        {
            return match *method {
                Method::POST => handle_admin_store_rotate_key(state, principal, name).await,
                _ => error_response(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "POST"),
            };
        }
        if let Some(name) = rest.strip_suffix("/unlock")
            && !name.is_empty()
            && !name.contains('/')
        {
            return match *method {
                Method::POST => handle_admin_store_unlock(state, principal, name, request).await,
                _ => error_response(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "POST"),
            };
        }
        if let Some(name) = rest.strip_suffix("/lock")
            && !name.is_empty()
            && !name.contains('/')
        {
            return match *method {
                Method::POST => handle_admin_store_lock(state, principal, name),
                _ => error_response(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "POST"),
            };
        }
        if !rest.is_empty() && !rest.contains('/') {
            return match *method {
                Method::GET => handle_admin_store_detail(state, rest),
                Method::PUT => handle_admin_store_update(state, principal, rest, request).await,
                Method::DELETE => {
                    let force = request
                        .uri()
                        .query()
                        .map(|q| {
                            q.split('&')
                                .any(|pair| matches!(pair, "force=true" | "force=1" | "force"))
                        })
                        .unwrap_or(false);
                    handle_admin_store_delete(state, principal, rest, force)
                }
                _ => error_response(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "method_not_allowed",
                    "GET, PUT, or DELETE",
                ),
            };
        }
    }
    error_response(StatusCode::NOT_FOUND, "not_found", "unknown admin endpoint")
}
