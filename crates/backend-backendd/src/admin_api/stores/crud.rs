//! Store admin CRUD: `list`, `create`, `update`, `delete`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use serde::Deserialize;
use underwrap_protocol::config::StoreRecord;
use underwrap_protocol::service::{
    AdminStoreBackendKind, AdminStoreCreateRequest, AdminStoreCreateResponse,
    AdminStoreDeleteResponse, AdminStoreListItem, AdminStoreUpdateRequest, AdminStoreUsageSummary,
    AdminStoresListResponse, ServiceErrorResponse,
};
use underwrap_store::StorageBackend;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::http::responses::Body;
use crate::runtime_state::AdminRuntimeState;
use crate::store_registry::{LoadedStoreRecord, StoreState, save_store_records};
use crate::users::Principal;

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_stores_list, handle_admin_store_create))
        .routes(routes!(
            handle_admin_store_update,
            handle_admin_store_delete
        ))
}

use super::{
    admin_store_actions, admin_store_list_item, admin_store_usage, admin_store_usage_for_locked,
    create_data_dir_secure, generate_master_key_bytes, master_key_hex, resolve_store_dir,
};
use crate::admin_api::audit::{AuditEntity, log_admin_event};

#[utoipa::path(
    get,
    path = "/v1/admin/stores",
    tag = "stores",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "Every store in the registry plus per-store usage projections.", body = AdminStoresListResponse),
    ),
)]
pub(crate) async fn handle_admin_stores_list(
    State(state): State<Arc<AdminRuntimeState>>,
) -> Result<Json<AdminStoresListResponse>, AdminError> {
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
        let (usage, locked) = match handle.as_deref() {
            Some(b) => (admin_store_usage(&state, &id, b)?, false),
            None => (admin_store_usage_for_locked(&state, &id), true),
        };
        let actions = admin_store_actions(&loaded, &usage, locked);
        items.push(admin_store_list_item(&loaded, locked, usage, actions));
    }
    items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(Json(AdminStoresListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/stores",
    tag = "stores",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminStoreCreateRequest,
    responses(
        (status = 200, description = "Store created and added to the registry.", body = AdminStoreCreateResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 409, description = "Store name already exists.", body = ServiceErrorResponse),
        (status = 500, description = "Backend could not persist the registry.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_create(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminStoreCreateRequest>,
) -> Result<Json<AdminStoreCreateResponse>, AdminError> {
    let visible_name = body.name.trim();
    if visible_name.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_store_name",
            "store name must not be empty",
        ));
    }
    let visible_name = visible_name.to_string();

    let store_id = crate::ids::fresh_id("store", |candidate| {
        state.stores.read().unwrap().records.contains_key(candidate)
    });

    let validated_dir = resolve_store_dir(&state, &store_id)
        .map_err(|(code, msg)| AdminError::bad_request(code, msg))?;

    {
        let reg = state.stores.read().unwrap();
        if reg
            .records
            .values()
            .any(|existing| existing.record.name.eq_ignore_ascii_case(&visible_name))
        {
            return Err(AdminError::conflict(
                "store_name_already_exists",
                format!("store name '{visible_name}' already exists"),
            ));
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
            create_data_dir_secure(&validated_dir).map_err(|e| {
                AdminError::internal_with_code(
                    "create_data_dir_failed",
                    format!("creating data dir {}: {e:#}", validated_dir.display()),
                )
            })?;
            let backend: Arc<dyn StorageBackend> = underwrap_store::open(&storage)
                .map(Arc::from)
                .map_err(|e| {
                AdminError::internal_with_code(
                    "open_store_failed",
                    format!("opening store backend after create: {e:#}"),
                )
            })?;
            (storage, backend, None)
        }
        AdminStoreBackendKind::Encrypted => {
            create_data_dir_secure(&validated_dir).map_err(|e| {
                AdminError::internal_with_code(
                    "create_data_dir_failed",
                    format!("creating data dir {}: {e:#}", validated_dir.display()),
                )
            })?;
            let key_bytes = generate_master_key_bytes();
            let key_hex = master_key_hex(&key_bytes);
            let backend =
                underwrap_store::EncryptedBackend::open_with_key(&validated_dir, key_bytes)
                    .map_err(|e| {
                        AdminError::internal_with_code(
                            "open_store_failed",
                            format!("opening encrypted store backend after create: {e:#}"),
                        )
                    })?;
            backend
                .write(
                    underwrap_store::ENCRYPTED_CANARY_ID,
                    underwrap_store::ENCRYPTED_CANARY_PLAINTEXT,
                )
                .map_err(|e| {
                    AdminError::internal_with_code(
                        "canary_write_failed",
                        format!("writing unlock canary for new encrypted store: {e:#}"),
                    )
                })?;
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
            return Err(AdminError::internal_with_code(
                "write_store_registry_failed",
                format!("{e:#}"),
            ));
        }
    }
    log_admin_event(
        "store.create",
        &principal,
        AuditEntity::Store { store: &store_id },
        None,
        "success",
    );
    let usage = AdminStoreUsageSummary {
        mapped_client_instance_count: 0,
        has_secrets: false,
    };
    let actions = admin_store_actions(&loaded, &usage, false);
    Ok(Json(AdminStoreCreateResponse {
        store: admin_store_list_item(&loaded, false, usage, actions),
        key_hex: generated_key_hex,
    }))
}

#[utoipa::path(
    put,
    path = "/v1/admin/stores/{id}",
    tag = "stores",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    request_body = AdminStoreUpdateRequest,
    responses(
        (status = 200, description = "Store updated.", body = AdminStoreListItem),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_update(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
    Body(body): Body<AdminStoreUpdateRequest>,
) -> Result<Json<AdminStoreListItem>, AdminError> {
    let updated = {
        let reg = state.stores.read().unwrap();
        let loaded = reg
            .records
            .get(&name)
            .ok_or_else(AdminError::store_not_found)?;
        let mut record = loaded.record.clone();
        if let Some(new_name) = &body.name {
            let new_name = new_name.trim();
            if new_name.is_empty() {
                return Err(AdminError::bad_request(
                    "invalid_store_name",
                    "store name must not be empty",
                ));
            }
            let new_name = new_name.to_string();
            if reg.records.iter().any(|(other_id, other)| {
                other_id != &name && other.record.name.eq_ignore_ascii_case(&new_name)
            }) {
                return Err(AdminError::conflict(
                    "store_name_already_exists",
                    format!("store name '{new_name}' already exists"),
                ));
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
        if let Some(loaded) = reg.records.get_mut(&name) {
            loaded.record = updated.clone();
        }
        if let Err(e) = save_store_records(&state.stores_file, &reg.records) {
            return Err(AdminError::internal_with_code(
                "write_store_registry_failed",
                format!("{e:#}"),
            ));
        }
    }
    log_admin_event(
        "store.update",
        &principal,
        AuditEntity::Store { store: &name },
        None,
        "success",
    );
    let loaded = LoadedStoreRecord { record: updated };
    let (handle, locked) = {
        let reg = state.stores.read().unwrap();
        match reg.state.get(&name) {
            Some(StoreState::Unlocked(b)) => (Some(Arc::clone(b)), false),
            Some(StoreState::Locked) => (None, true),
            None => {
                return Err(AdminError::internal_with_code(
                    "internal",
                    "store metadata present but registry state missing",
                ));
            }
        }
    };
    let usage = match handle.as_deref() {
        Some(b) => admin_store_usage(&state, &name, b)?,
        None => admin_store_usage_for_locked(&state, &name),
    };
    let actions = admin_store_actions(&loaded, &usage, locked);
    Ok(Json(admin_store_list_item(&loaded, locked, usage, actions)))
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DeleteQuery {
    #[serde(default)]
    pub(crate) force: Option<String>,
}

#[utoipa::path(
    delete,
    path = "/v1/admin/stores/{id}",
    tag = "stores",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("force" = Option<String>, Query, description = "Pass `true` / `1` / empty to bypass the empty-store guard."),
    ),
    responses(
        (status = 200, description = "Store removed.", body = AdminStoreDeleteResponse),
        (status = 400, description = "Store is non-empty and `force` was not set.", body = ServiceErrorResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<AdminStoreDeleteResponse>, AdminError> {
    let force = query
        .force
        .as_deref()
        .is_some_and(|v| matches!(v, "true" | "1" | ""));

    let (store_handle, dir_for_hint, locked) = {
        let reg = state.stores.read().unwrap();
        let loaded = reg
            .records
            .get(&name)
            .ok_or_else(AdminError::store_not_found)?;
        let dir = loaded.record.storage.dir().to_path_buf();
        match reg.state.get(&name) {
            Some(StoreState::Unlocked(b)) => (Some(Arc::clone(b)), dir, false),
            Some(StoreState::Locked) => (None, dir, true),
            None => {
                return Err(AdminError::internal_with_code(
                    "internal",
                    "store metadata present but registry state missing",
                ));
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
        return Err(AdminError::conflict(
            "store_in_use",
            "store is still mapped to one or more client instances",
        ));
    }

    let non_empty = match store_handle.as_deref() {
        Some(h) => h
            .list("")
            .map_err(|e| {
                AdminError::internal_with_code(
                    "list_store_failed",
                    format!("checking store contents before delete: {e:#}"),
                )
            })?
            .iter()
            .any(|id| id != underwrap_store::ENCRYPTED_CANARY_ID),
        // Locked encrypted store: we cannot inspect contents. Refuse
        // unless the caller passed `force=true`, since deleting a
        // store without confirming it's empty risks orphaning data
        // the operator forgot about.
        None => true,
    };
    if non_empty && !force {
        let (code, message) = if locked {
            (
                "store_locked_use_force",
                "store is locked; contents cannot be inspected. re-issue with `?force=true` to delete the record anyway",
            )
        } else {
            (
                "store_not_empty",
                "store contains secrets; re-issue with `?force=true` to delete the record anyway",
            )
        };
        return Err(AdminError::conflict(code, message));
    }

    {
        let mut reg = state.stores.write().unwrap();
        reg.state.remove(&name);
        reg.records.remove(&name);
        if let Err(e) = save_store_records(&state.stores_file, &reg.records) {
            return Err(AdminError::internal_with_code(
                "write_store_registry_failed",
                format!("{e:#}"),
            ));
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
    let operation = if non_empty {
        Some("force-deleted")
    } else {
        None
    };
    log_admin_event(
        "store.delete",
        &principal,
        AuditEntity::Store { store: &name },
        operation,
        "success",
    );
    Ok(Json(AdminStoreDeleteResponse {
        ok: true,
        cleanup_hint,
    }))
}

#[cfg(test)]
mod tests;
