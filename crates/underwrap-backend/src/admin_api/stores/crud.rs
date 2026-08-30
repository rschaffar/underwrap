//! Store admin CRUD: `list`, `create`, `update`, `delete`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use serde::Deserialize;
use underwrap_backend_store::StorageBackend;
use underwrap_protocol::config::StoreRecord;
use underwrap_protocol::service::{
    AdminStoreBackendKind, AdminStoreCreateRequest, AdminStoreCreateResponse,
    AdminStoreDeleteCleanupHint, AdminStoreDeleteResponse, AdminStoreListItem,
    AdminStoreUpdateRequest, AdminStoreUsageSummary, AdminStoresListResponse, ServiceErrorResponse,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::csrf::Principal;
use crate::http::responses::Body;
use crate::runtime_state::ListenerState;
use crate::store_registry::{
    RegistryLookupError, StoreEntry, StoreState, delete_store_record, save_store_record,
};

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<ListenerState>> {
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
    State(state): State<Arc<ListenerState>>,
) -> Result<Json<AdminStoresListResponse>, AdminError> {
    #[allow(clippy::type_complexity)]
    let visible: Vec<(StoreRecord, Option<Arc<dyn StorageBackend>>)> = state
        .shared
        .stores
        .read()
        .unwrap()
        .entries
        .values()
        .map(|entry| (entry.record.clone(), entry.state.backend().cloned()))
        .collect();
    let mut items = Vec::with_capacity(visible.len());
    for (record, handle) in visible {
        let (usage, locked) = match handle.as_deref() {
            Some(b) => (admin_store_usage(&state, &record.id, b)?, false),
            None => (admin_store_usage_for_locked(&state, &record.id), true),
        };
        let actions = admin_store_actions(&record, &usage, locked);
        items.push(admin_store_list_item(&record, locked, usage, actions));
    }
    items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(Json(AdminStoresListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/stores",
    tag = "stores",
    security(("session_cookie" = [], "csrf_header" = [])),
    request_body = AdminStoreCreateRequest,
    responses(
        (status = 200, description = "Store created and added to the registry.", body = AdminStoreCreateResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 409, description = "Store name already exists.", body = ServiceErrorResponse),
        (status = 500, description = "Backend could not persist the registry.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_create(
    State(state): State<Arc<ListenerState>>,
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
        state
            .shared
            .stores
            .read()
            .unwrap()
            .entries
            .contains_key(candidate)
    });

    let validated_dir = resolve_store_dir(&state, &store_id)
        .map_err(|(code, msg)| AdminError::bad_request(code, msg))?;

    {
        let reg = state.shared.stores.read().unwrap();
        if reg
            .entries
            .values()
            .any(|entry| entry.record.name.eq_ignore_ascii_case(&visible_name))
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
                AdminError::internal_with_source(
                    "create_data_dir_failed",
                    "could not create store data directory",
                    "creating plain store data directory",
                    e,
                )
            })?;
            let backend: Arc<dyn StorageBackend> = underwrap_backend_store::open(&storage)
                .map(Arc::from)
                .map_err(|e| {
                    AdminError::internal_with_source(
                        "open_store_failed",
                        "could not open store",
                        "opening plain store after creation",
                        e,
                    )
                })?;
            (storage, backend, None)
        }
        AdminStoreBackendKind::Encrypted => {
            let parent = validated_dir.parent().ok_or_else(|| {
                AdminError::internal_with_code(
                    "create_data_dir_failed",
                    "encrypted store directory has no parent",
                )
            })?;
            create_data_dir_secure(parent).map_err(|e| {
                AdminError::internal_with_source(
                    "create_data_dir_failed",
                    "could not create store data directory",
                    "creating encrypted stores data root",
                    e,
                )
            })?;
            let mut key_bytes = generate_master_key_bytes();
            let key_hex = master_key_hex(&key_bytes);
            let backend_result = underwrap_backend_store::EncryptedBackend::create_with_key(
                &validated_dir,
                key_bytes,
            );
            key_bytes.fill(0);
            let backend = backend_result.map_err(|e| {
                AdminError::internal_with_source(
                    "open_store_failed",
                    "could not open store",
                    "creating encrypted store backend",
                    e,
                )
            })?;
            backend
                .write(
                    underwrap_backend_store::ENCRYPTED_CANARY_ID,
                    underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT,
                )
                .map_err(|e| {
                    AdminError::internal_with_source(
                        "canary_write_failed",
                        "could not initialize encrypted store",
                        "writing unlock canary for new encrypted store",
                        e,
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
        created_at: Some(crate::time::now_epoch_secs()),
        storage,
    };

    {
        let mut reg = state.shared.stores.write().unwrap();
        reg.entries.insert(
            store_id.clone(),
            StoreEntry::new(record.clone(), StoreState::Unlocked(opened)),
        );
        if let Err(e) = save_store_record(&state.admin().stores_dir, &record) {
            reg.entries.remove(&store_id);
            return Err(AdminError::internal_with_source(
                "write_store_registry_failed",
                "could not persist store registry",
                "persisting created store registry record",
                e,
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
    let actions = admin_store_actions(&record, &usage, false);
    Ok(Json(AdminStoreCreateResponse {
        store: admin_store_list_item(&record, false, usage, actions),
        key_hex: generated_key_hex,
    }))
}

#[utoipa::path(
    put,
    path = "/v1/admin/stores/{id}",
    tag = "stores",
    security(("session_cookie" = [], "csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    request_body = AdminStoreUpdateRequest,
    responses(
        (status = 200, description = "Store updated.", body = AdminStoreListItem),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_update(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
    Body(body): Body<AdminStoreUpdateRequest>,
) -> Result<Json<AdminStoreListItem>, AdminError> {
    let updated = {
        let mut reg = state.shared.stores.write().unwrap();
        let entry = reg
            .entry(&name)
            .map_err(|_| AdminError::store_not_found())?;
        let mut updated = entry.record.clone();
        if let Some(new_name) = &body.name {
            let new_name = new_name.trim();
            if new_name.is_empty() {
                return Err(AdminError::bad_request(
                    "invalid_store_name",
                    "store name must not be empty",
                ));
            }
            let new_name = new_name.to_string();
            if reg.entries.iter().any(|(other_id, other)| {
                other_id != &name && other.record.name.eq_ignore_ascii_case(&new_name)
            }) {
                return Err(AdminError::conflict(
                    "store_name_already_exists",
                    format!("store name '{new_name}' already exists"),
                ));
            }
            updated.name = new_name;
        }
        if let Some(new_desc) = &body.description {
            updated.description = if new_desc.is_empty() {
                None
            } else {
                Some(new_desc.clone())
            };
        }
        let entry = reg
            .entries
            .get_mut(&name)
            .ok_or_else(AdminError::store_not_found)?;
        let published = updated.clone();
        if let Err(e) = save_store_record(&state.admin().stores_dir, &updated) {
            return Err(AdminError::internal_with_source(
                "write_store_registry_failed",
                "could not persist store registry",
                "persisting updated store registry record",
                e,
            ));
        }
        entry.record = published;
        updated
    };
    log_admin_event(
        "store.update",
        &principal,
        AuditEntity::Store { store: &name },
        None,
        "success",
    );
    let (handle, locked) = {
        let reg = state.shared.stores.read().unwrap();
        let entry = reg.entry(&name).map_err(|_| {
            AdminError::internal_with_code(
                "internal",
                "store vanished from registry between update and read-back",
            )
        })?;
        let handle = entry.state.backend().cloned();
        let locked = handle.is_none();
        (handle, locked)
    };
    let usage = match handle.as_deref() {
        Some(b) => admin_store_usage(&state, &name, b)?,
        None => admin_store_usage_for_locked(&state, &name),
    };
    let actions = admin_store_actions(&updated, &usage, locked);
    Ok(Json(admin_store_list_item(
        &updated, locked, usage, actions,
    )))
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DeleteQuery {
    #[serde(default)]
    pub(crate) force: bool,
}

#[utoipa::path(
    delete,
    path = "/v1/admin/stores/{id}",
    tag = "stores",
    security(("session_cookie" = [], "csrf_header" = [])),
    params(
        ("id" = String, Path, description = "Store id."),
        ("force" = bool, Query, description = "Pass `?force=true` to bypass the empty-store guard."),
    ),
    responses(
        (status = 200, description = "Store removed.", body = AdminStoreDeleteResponse),
        (status = 400, description = "Store is non-empty and `force` was not set.", body = ServiceErrorResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_delete(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<AdminStoreDeleteResponse>, AdminError> {
    let force = query.force;

    let (store_handle, dir_for_hint, locked) = {
        let reg = state.shared.stores.read().unwrap();
        let entry = reg.entry(&name).map_err(|e| match e {
            RegistryLookupError::NotFound => AdminError::store_not_found(),
            RegistryLookupError::Locked => AdminError::store_locked(&name),
        })?;
        let dir = entry.record.storage.dir().to_path_buf();
        let store_handle = entry.state.backend().cloned();
        let locked = store_handle.is_none();
        (store_handle, dir, locked)
    };
    if state
        .shared
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
                AdminError::internal_with_source(
                    "list_store_failed",
                    "could not inspect store contents",
                    "checking store contents before delete",
                    e,
                )
            })?
            .iter()
            .any(|id| !underwrap_protocol::control::is_control_key(id)),
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
        let mut reg = state.shared.stores.write().unwrap();
        if let Err(e) = delete_store_record(&state.admin().stores_dir, &name) {
            return Err(AdminError::internal_with_source(
                "write_store_registry_failed",
                "could not persist store registry",
                "deleting store registry record",
                e,
            ));
        }
        reg.entries.remove(&name);
    }
    let cleanup_hint = if non_empty {
        let data_dir = dir_for_hint.display().to_string();
        Some(AdminStoreDeleteCleanupHint {
            command: format!("rm -rf {data_dir}"),
            data_dir,
        })
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
