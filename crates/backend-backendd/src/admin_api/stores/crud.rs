//! Store admin CRUD: `list`, `detail`, `create`, `update`, `delete`.

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use std::sync::Arc;

use underwrap_protocol::config::StoreRecord;
use underwrap_protocol::service::{
    AdminStoreBackendKind, AdminStoreCreateRequest, AdminStoreCreateResponse,
    AdminStoreDeleteResponse, AdminStoreUpdateRequest, AdminStoreUsageSummary,
    AdminStoresListResponse,
};
use underwrap_store::StorageBackend;

use crate::http::responses::{error_response, json_response, parse_json_body};
use crate::runtime_state::AdminRuntimeState;
use crate::store_registry::{LoadedStoreRecord, StoreState, save_store_records};

use super::{
    admin_store_actions, admin_store_detail, admin_store_list_item, admin_store_usage,
    admin_store_usage_for_locked, create_data_dir_secure, generate_master_key_bytes,
    master_key_hex, resolve_store_dir, write_admin_store_audit,
};

pub(super) fn handle_admin_stores_list(state: &AdminRuntimeState) -> Response<Full<Bytes>> {
    #[allow(clippy::type_complexity)]
    let visible: Vec<(String, LoadedStoreRecord, Option<Arc<dyn StorageBackend>>)> = {
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

pub(super) fn handle_admin_store_detail(
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

pub(super) async fn handle_admin_store_create_endpoint(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminStoreCreateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    handle_admin_store_create(state, principal, &body)
}

/// Body-already-parsed half of [`handle_admin_store_create_endpoint`].
/// Split out so unit tests can drive it without synthesising a
/// `Request<Incoming>` (which `hyper` doesn't expose constructors
/// for). The HTTP-shaped handler above is exercised by Playwright.
pub(crate) fn handle_admin_store_create(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: &AdminStoreCreateRequest,
) -> Response<Full<Bytes>> {
    let visible_name = body.name.trim();
    if visible_name.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_store_name",
            "store name must not be empty",
        );
    }
    let visible_name = visible_name.to_string();

    let store_id = crate::ids::fresh_id("store", |candidate| {
        state.stores.read().unwrap().records.contains_key(candidate)
    });

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

pub(super) async fn handle_admin_store_update_endpoint(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminStoreUpdateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    handle_admin_store_update(state, principal, name, &body)
}

pub(crate) fn handle_admin_store_update(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    name: &str,
    body: &AdminStoreUpdateRequest,
) -> Response<Full<Bytes>> {
    let updated = {
        let reg = state.stores.read().unwrap();
        let Some(loaded) = reg.records.get(name) else {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown store");
        };
        let mut record = loaded.record.clone();
        if let Some(new_name) = &body.name {
            let new_name = new_name.trim();
            if new_name.is_empty() {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_store_name",
                    "store name must not be empty",
                );
            }
            let new_name = new_name.to_string();
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
        if let Some(new_desc) = &body.description {
            record.description = if new_desc.is_empty() {
                None
            } else {
                Some(new_desc.clone())
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

#[cfg(test)]
mod tests;
