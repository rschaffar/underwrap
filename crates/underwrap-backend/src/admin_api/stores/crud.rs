//! Store admin CRUD: `list`, `create`, `update`, `delete`.

mod cleanup_hint;

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use serde::Deserialize;
use underwrap_backend_store::StorageBackend;
use underwrap_protocol::config::StoreRecord;
use underwrap_protocol::service::{
    AdminStoreBackendKind, AdminStoreCreateRequest, AdminStoreCreateResponse,
    AdminStoreDeleteResponse, AdminStoreLifecycleState, AdminStoreListItem,
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

type InitialStoreState = fn(Arc<dyn StorageBackend>) -> StoreState;

struct SubmittedCreationKey([u8; 32]);

impl SubmittedCreationKey {
    fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn copy_bytes(&self) -> [u8; 32] {
        self.0
    }
}

impl Drop for SubmittedCreationKey {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

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
    create_data_dir_secure, lifecycle_state, resolve_store_dir,
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
    let ids = state
        .shared
        .stores
        .read()
        .unwrap()
        .entries
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        let (record, lifecycle, lease) = {
            let registry = state.shared.stores.read().unwrap();
            let entry = registry
                .entry(&id)
                .map_err(|_| AdminError::store_not_found())?;
            let lifecycle = lifecycle_state(&entry.state);
            let lease = match lifecycle {
                AdminStoreLifecycleState::Ready | AdminStoreLifecycleState::Unlocked => {
                    Some(registry.lease(&id).map_err(|_| AdminError::store_busy())?)
                }
                AdminStoreLifecycleState::Locked
                | AdminStoreLifecycleState::Busy
                | AdminStoreLifecycleState::RecoveryRequired => None,
            };
            (entry.record.clone(), lifecycle, lease)
        };
        let usage = match lease.as_ref() {
            Some(lease) => admin_store_usage(&state, &record.id, lease.as_ref())?,
            None => admin_store_usage_for_locked(&state, &record.id),
        };
        let actions = admin_store_actions(lifecycle, &usage);
        items.push(admin_store_list_item(&record, lifecycle, usage, actions));
    }
    items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(Json(AdminStoresListResponse { items }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/stores",
    tag = "stores",
    description = "Create a store. Encrypted creation requires `key_hex` as exactly 64 lowercase hexadecimal characters without trimming or normalization; plain creation requires it to be omitted. Explicit JSON `null` is invalid for every backend kind.",
    security(("session_cookie" = [], "csrf_header" = [])),
    request_body = AdminStoreCreateRequest,
    responses(
        (status = 200, description = "Store created and added to the registry.", body = AdminStoreCreateResponse),
        (status = 400, description = "Validation failed, including an explicit null `key_hex` (`invalid_request`) or conditional key errors (`key_required`, `unexpected_key`, or `invalid_key_hex`).", body = ServiceErrorResponse),
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

    let mut submitted_key = match (body.backend, body.key_hex.as_ref()) {
        (AdminStoreBackendKind::Plain, Some(_)) => {
            return Err(AdminError::bad_request(
                "unexpected_key",
                "plain store creation does not accept a key",
            ));
        }
        (AdminStoreBackendKind::Plain, None) => None,
        (AdminStoreBackendKind::Encrypted, None) => {
            return Err(AdminError::bad_request(
                "key_required",
                "encrypted store creation requires a key",
            ));
        }
        (AdminStoreBackendKind::Encrypted, Some(key)) => Some(SubmittedCreationKey::new(
            underwrap_backend_store::parse_hex_key_strict(key.expose()).map_err(|_| {
                AdminError::bad_request(
                    "invalid_key_hex",
                    "key must be exactly 64 lowercase hexadecimal characters",
                )
            })?,
        )),
    };

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

    let _store_collection_guard = state.admin().store_collection_lock.lock().unwrap();
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

    let parent = validated_dir.parent().ok_or_else(|| {
        AdminError::internal_with_code("create_data_dir_failed", "store directory has no parent")
    })?;
    create_data_dir_secure(parent).map_err(|e| {
        AdminError::internal_with_source(
            "create_data_dir_failed",
            "could not create store data directory",
            "creating stores data root",
            e,
        )
    })?;

    let (storage, opened, initial_state): (
        underwrap_protocol::config::StorageConfig,
        Arc<dyn StorageBackend>,
        InitialStoreState,
    ) = match body.backend {
        AdminStoreBackendKind::Plain => {
            let backend: Arc<dyn StorageBackend> = Arc::new(
                underwrap_backend_store::PlainBackend::create(&validated_dir).map_err(|e| {
                    AdminError::internal_with_source(
                        "open_store_failed",
                        "could not open store",
                        "creating plain store backend",
                        e,
                    )
                })?,
            );
            (
                underwrap_protocol::config::StorageConfig::Plain {
                    dir: validated_dir.clone(),
                },
                backend,
                StoreState::Ready,
            )
        }
        AdminStoreBackendKind::Encrypted => {
            let mut key_bytes = submitted_key
                .take()
                .expect("encrypted key was validated before filesystem mutation")
                .copy_bytes();
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
            (
                underwrap_protocol::config::StorageConfig::Encrypted {
                    dir: validated_dir.clone(),
                },
                Arc::new(backend),
                StoreState::Unlocked,
            )
        }
    };

    let record = StoreRecord {
        id: store_id.clone(),
        name: visible_name,
        description: body.description.clone(),
        created_at: Some(crate::time::now_epoch_secs()),
        storage,
    };

    if let Err(e) = save_store_record(&state.admin().stores_dir, &record) {
        return Err(AdminError::internal_with_source(
            "write_store_registry_failed",
            "could not persist store registry",
            "persisting created store registry record",
            e,
        ));
    }
    state.shared.stores.write().unwrap().entries.insert(
        store_id.clone(),
        StoreEntry::new(record.clone(), initial_state(opened)),
    );
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
    let lifecycle = match body.backend {
        AdminStoreBackendKind::Plain => AdminStoreLifecycleState::Ready,
        AdminStoreBackendKind::Encrypted => AdminStoreLifecycleState::Unlocked,
    };
    let actions = admin_store_actions(lifecycle, &usage);
    Ok(Json(AdminStoreCreateResponse {
        store: admin_store_list_item(&record, lifecycle, usage, actions),
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
        (status = 500, description = "Trusted-host migration recovery is required (`migration_recovery_required`).", body = ServiceErrorResponse),
        (status = 503, description = "Store is busy (`store_busy`).", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_update(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
    Body(body): Body<AdminStoreUpdateRequest>,
) -> Result<Json<AdminStoreListItem>, AdminError> {
    let lifecycle_lock = {
        let registry = state.shared.stores.read().unwrap();
        Arc::clone(
            &registry
                .entry(&name)
                .map_err(|_| AdminError::store_not_found())?
                .lifecycle_lock,
        )
    };
    let _lifecycle_guard = lifecycle_lock.lock().await;
    let _collection_guard = state.admin().store_collection_lock.lock().unwrap();
    let (updated, identity, generation, original) = {
        let registry = state.shared.stores.read().unwrap();
        let entry = registry
            .entry(&name)
            .map_err(|_| AdminError::store_not_found())?;
        if !Arc::ptr_eq(&entry.lifecycle_lock, &lifecycle_lock) {
            return Err(AdminError::store_busy());
        }
        match entry.state {
            StoreState::Busy => return Err(AdminError::store_busy()),
            StoreState::RecoveryRequired => {
                return Err(AdminError::migration_recovery_required());
            }
            StoreState::Ready(_) | StoreState::Unlocked(_) | StoreState::Locked => {}
        }
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
            if registry.entries.iter().any(|(other_id, other)| {
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
        (
            updated,
            Arc::clone(&entry.identity),
            entry.generation(),
            entry.record.clone(),
        )
    };
    if let Err(e) = save_store_record(&state.admin().stores_dir, &updated) {
        return Err(AdminError::internal_with_source(
            "write_store_registry_failed",
            "could not persist store registry",
            "persisting updated store registry record",
            e,
        ));
    }
    {
        let mut registry = state.shared.stores.write().unwrap();
        let entry = registry
            .entries
            .get_mut(&name)
            .ok_or_else(AdminError::store_not_found)?;
        if !Arc::ptr_eq(&entry.identity, &identity)
            || !Arc::ptr_eq(&entry.lifecycle_lock, &lifecycle_lock)
            || entry.generation() != generation
            || entry.record != original
        {
            return Err(AdminError::migration_recovery_required());
        }
        entry.record = updated.clone();
    }
    log_admin_event(
        "store.update",
        &principal,
        AuditEntity::Store { store: &name },
        None,
        "success",
    );
    let (lifecycle, lease) = {
        let registry = state.shared.stores.read().unwrap();
        let entry = registry.entry(&name).map_err(|_| {
            AdminError::internal_with_code(
                "internal",
                "store vanished from registry between update and read-back",
            )
        })?;
        let lifecycle = lifecycle_state(&entry.state);
        let lease = match lifecycle {
            AdminStoreLifecycleState::Ready | AdminStoreLifecycleState::Unlocked => Some(
                registry
                    .lease(&name)
                    .map_err(|_| AdminError::store_busy())?,
            ),
            _ => None,
        };
        (lifecycle, lease)
    };
    let usage = match lease.as_ref() {
        Some(lease) => admin_store_usage(&state, &name, lease.as_ref())?,
        None => admin_store_usage_for_locked(&state, &name),
    };
    let actions = admin_store_actions(lifecycle, &usage);
    Ok(Json(admin_store_list_item(
        &updated, lifecycle, usage, actions,
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
        ("force" = Option<bool>, Query, description = "Optional; defaults to false. Pass `?force=true` to remove a nonempty or locked store record. Mapped runtime clients still block deletion; data directories are retained."),
    ),
    responses(
        (status = 200, description = "Store record removed; the data directory and retained generations are not deleted.", body = AdminStoreDeleteResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 409, description = "Store has mapped runtime clients (`store_in_use`), or force was omitted for a nonempty (`store_not_empty`) or locked (`store_locked_use_force`) store.", body = ServiceErrorResponse),
        (status = 500, description = "Trusted-host migration recovery is required (`migration_recovery_required`).", body = ServiceErrorResponse),
        (status = 503, description = "Store is busy (`store_busy`).", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_delete(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<AdminStoreDeleteResponse>, AdminError> {
    let force = query.force;
    let lifecycle_lock = {
        let registry = state.shared.stores.read().unwrap();
        Arc::clone(
            &registry
                .entry(&name)
                .map_err(|_| AdminError::store_not_found())?
                .lifecycle_lock,
        )
    };
    let _lifecycle_guard = lifecycle_lock.lock().await;
    let _collection_guard = state.admin().store_collection_lock.lock().unwrap();

    let (store_lease, dir_for_hint, lifecycle, identity) = {
        let registry = state.shared.stores.read().unwrap();
        let entry = registry
            .entry(&name)
            .map_err(|_| AdminError::store_not_found())?;
        if !Arc::ptr_eq(&entry.lifecycle_lock, &lifecycle_lock) {
            return Err(AdminError::store_busy());
        }
        let lifecycle = lifecycle_state(&entry.state);
        let lease = match lifecycle {
            AdminStoreLifecycleState::Ready | AdminStoreLifecycleState::Unlocked => {
                Some(registry.lease(&name).map_err(|error| match error {
                    RegistryLookupError::NotFound => AdminError::store_not_found(),
                    RegistryLookupError::Locked => AdminError::store_locked(&name),
                    RegistryLookupError::Busy => AdminError::store_busy(),
                    RegistryLookupError::RecoveryRequired => {
                        AdminError::migration_recovery_required()
                    }
                })?)
            }
            AdminStoreLifecycleState::Locked => None,
            AdminStoreLifecycleState::Busy => return Err(AdminError::store_busy()),
            AdminStoreLifecycleState::RecoveryRequired => {
                return Err(AdminError::migration_recovery_required());
            }
        };
        (
            lease,
            entry.record.storage.dir().to_path_buf(),
            lifecycle,
            Arc::clone(&entry.identity),
        )
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

    let non_empty = match store_lease.as_ref() {
        Some(lease) => lease
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
        let (code, message) = if lifecycle == AdminStoreLifecycleState::Locked {
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
        let registry = state.shared.stores.read().unwrap();
        let entry = registry
            .entry(&name)
            .map_err(|_| AdminError::store_not_found())?;
        if !Arc::ptr_eq(&entry.identity, &identity)
            || !Arc::ptr_eq(&entry.lifecycle_lock, &lifecycle_lock)
        {
            return Err(AdminError::store_busy());
        }
        match entry.state {
            StoreState::Busy => return Err(AdminError::store_busy()),
            StoreState::RecoveryRequired => {
                return Err(AdminError::migration_recovery_required());
            }
            StoreState::Ready(_) | StoreState::Unlocked(_) | StoreState::Locked => {}
        }
    }
    // Prepare exact manual guidance before removing registry ownership: a lossy
    // or unresolved pathname must never become an operator's removal command.
    let cleanup_hint = non_empty
        .then(|| cleanup_hint::for_directory(&dir_for_hint))
        .transpose()?;
    if let Err(e) = delete_store_record(&state.admin().stores_dir, &name) {
        return Err(AdminError::internal_with_source(
            "write_store_registry_failed",
            "could not persist store registry",
            "deleting store registry record",
            e,
        ));
    }
    state.shared.stores.write().unwrap().entries.remove(&name);
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
