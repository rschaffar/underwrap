//! Encrypted-store lifecycle: `unlock`, `lock`, `rotate-key`.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::{HeaderMap, HeaderValue, header};
use underwrap_backend_store::StorageBackend;
use underwrap_protocol::service::{
    AdminKeyHex, AdminOkResponse, AdminStoreKeyResponse, AdminStoreLockResponse,
    AdminStoreMigrateRequest, AdminStoreMigrateResponse, AdminStoreRotateKeyResponse,
    AdminStoreUnlockRequest, ServiceErrorResponse,
};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::admin_api::AdminError;
use crate::admin_api::csrf::Principal;
use crate::http::responses::Body;
use crate::runtime_state::ListenerState;
use crate::store_registry::StoreState;

mod migration;
mod rotation;
mod workspace;

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<ListenerState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_store_key_generate))
        .routes(routes!(handle_admin_store_migrate))
        .routes(routes!(handle_admin_store_rotate_key))
        .routes(routes!(handle_admin_store_unlock))
        .routes(routes!(handle_admin_store_lock))
}

use crate::admin_api::audit::{AuditEntity, log_admin_event};

#[utoipa::path(
    post,
    path = "/v1/admin/store-keys",
    tag = "stores",
    security(("session_cookie" = [], "csrf_header" = [])),
    responses(
        (status = 200, description = "Fresh key generated without mutating storage; response carries exact no-cache headers.", body = AdminStoreKeyResponse,
            headers(
                ("Cache-Control" = String, description = "Always `no-store, no-cache, must-revalidate`."),
                ("Pragma" = String, description = "Always `no-cache`."),
                ("Expires" = String, description = "Always `0`.")
            )
        ),
    ),
)]
pub(crate) async fn handle_admin_store_key_generate(
    axum::Extension(principal): axum::Extension<Principal>,
) -> Result<(HeaderMap, Json<AdminStoreKeyResponse>), AdminError> {
    let mut key_bytes = super::generate_master_key_bytes();
    let key_hex = AdminKeyHex::new(super::master_key_hex(&key_bytes));
    key_bytes.fill(0);
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(header::EXPIRES, HeaderValue::from_static("0"));
    log_admin_event(
        "store.key-generate",
        &principal,
        AuditEntity::StoreKey,
        None,
        "success",
    );
    Ok((headers, Json(AdminStoreKeyResponse { key_hex })))
}

#[utoipa::path(
    post,
    path = "/v1/admin/stores/{id}/migrate",
    tag = "stores",
    description = "Migrate a store backend without changing its id. Migration to encrypted requires `key_hex` as exactly 64 lowercase hexadecimal characters without trimming or normalization; migration to plain requires it to be omitted.",
    security(("session_cookie" = [], "csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    request_body = AdminStoreMigrateRequest,
    responses(
        (status = 200, description = "Store migrated at its stable id.", body = AdminStoreMigrateResponse),
        (status = 400, description = "Direction-specific key validation failed (`key_required`, `unexpected_key`, or `invalid_key_hex`).", body = ServiceErrorResponse),
        (status = 404, description = "Store not found (`not_found`).", body = ServiceErrorResponse),
        (status = 409, description = "Requested backend matches the current backend (`same_backend`).", body = ServiceErrorResponse),
        (status = 500, description = "Migration failed (`migrate_stage_failed`, `migrate_list_failed`, `migrate_canary_failed`, `migrate_read_failed`, `migrate_write_failed`, `migrate_verify_failed`, `migrate_swap_failed`, `migrate_reopen_failed`, `write_store_registry_failed`, or `migration_recovery_required`).", body = ServiceErrorResponse),
        (status = 503, description = "Store is unavailable (`store_locked` or `store_busy`).", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_migrate(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
    Body(body): Body<AdminStoreMigrateRequest>,
) -> Result<Json<AdminStoreMigrateResponse>, AdminError> {
    migrate_with_hooks(state, principal, name, body, &migration::ProductionHooks).await
}

struct BusyCancellationGuard {
    state: Arc<ListenerState>,
    store_id: String,
    identity: Arc<()>,
    lifecycle_lock: Arc<tokio::sync::Mutex<()>>,
    source_generation: u64,
    source_backend: Option<Arc<dyn StorageBackend>>,
    source_kind: underwrap_protocol::service::AdminStoreBackendKind,
    active: bool,
}

impl BusyCancellationGuard {
    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for BusyCancellationGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut registry = self.state.shared.stores.write().unwrap();
        let Some(entry) = registry.entries.get_mut(&self.store_id) else {
            return;
        };
        if Arc::ptr_eq(&entry.identity, &self.identity)
            && Arc::ptr_eq(&entry.lifecycle_lock, &self.lifecycle_lock)
            && entry.generation() == self.source_generation
            && matches!(entry.state, StoreState::Busy)
        {
            let backend = self
                .source_backend
                .take()
                .expect("busy cancellation source backend is present");
            entry.state = match self.source_kind {
                underwrap_protocol::service::AdminStoreBackendKind::Plain => {
                    StoreState::Ready(backend)
                }
                underwrap_protocol::service::AdminStoreBackendKind::Encrypted => {
                    StoreState::Unlocked(backend)
                }
            };
        }
    }
}

async fn migrate_with_hooks(
    state: Arc<ListenerState>,
    principal: Principal,
    name: String,
    body: AdminStoreMigrateRequest,
    hooks: &dyn migration::MigrationHooks,
) -> Result<Json<AdminStoreMigrateResponse>, AdminError> {
    let (lifecycle_lock, identity, source_kind) = {
        let registry = state.shared.stores.read().unwrap();
        let entry = registry
            .entry(&name)
            .map_err(|_| AdminError::store_not_found())?;
        (
            Arc::clone(&entry.lifecycle_lock),
            Arc::clone(&entry.identity),
            super::backend_kind(&entry.record.storage),
        )
    };
    if source_kind == body.backend {
        return Err(AdminError::conflict(
            "same_backend",
            "the store already uses the requested backend",
        ));
    }
    let target_key = match (body.backend, body.key_hex.as_ref()) {
        (underwrap_protocol::service::AdminStoreBackendKind::Encrypted, None) => {
            return Err(AdminError::bad_request(
                "key_required",
                "plain-to-encrypted migration requires a key",
            ));
        }
        (underwrap_protocol::service::AdminStoreBackendKind::Encrypted, Some(key)) => {
            Some(migration::MigrationKey::new(
                underwrap_backend_store::parse_hex_key_strict(key.expose()).map_err(|_| {
                    AdminError::bad_request(
                        "invalid_key_hex",
                        "key must be exactly 64 lowercase hexadecimal characters",
                    )
                })?,
            ))
        }
        (underwrap_protocol::service::AdminStoreBackendKind::Plain, Some(_)) => {
            return Err(AdminError::bad_request(
                "unexpected_key",
                "encrypted-to-plain migration does not accept a key",
            ));
        }
        (underwrap_protocol::service::AdminStoreBackendKind::Plain, None) => None,
    };

    let _lifecycle_guard = lifecycle_lock.lock().await;
    let (source_record, target_record, source_backend, source_generation, leases) = {
        let mut registry = state.shared.stores.write().unwrap();
        let entry = registry
            .entries
            .get_mut(&name)
            .ok_or_else(AdminError::store_not_found)?;
        if !Arc::ptr_eq(&entry.identity, &identity)
            || !Arc::ptr_eq(&entry.lifecycle_lock, &lifecycle_lock)
            || super::backend_kind(&entry.record.storage) != source_kind
        {
            return Err(AdminError::store_busy());
        }
        let source_backend = match &entry.state {
            StoreState::Ready(backend)
                if source_kind == underwrap_protocol::service::AdminStoreBackendKind::Plain =>
            {
                Arc::clone(backend)
            }
            StoreState::Unlocked(backend)
                if source_kind == underwrap_protocol::service::AdminStoreBackendKind::Encrypted =>
            {
                Arc::clone(backend)
            }
            StoreState::Locked => return Err(AdminError::store_locked(&name)),
            StoreState::Busy => return Err(AdminError::store_busy()),
            StoreState::RecoveryRequired => return Err(AdminError::migration_recovery_required()),
            StoreState::Ready(_) | StoreState::Unlocked(_) => {
                return Err(AdminError::migration_recovery_required());
            }
        };
        let source_record = entry.record.clone();
        let mut target_record = source_record.clone();
        target_record.storage = match body.backend {
            underwrap_protocol::service::AdminStoreBackendKind::Plain => {
                underwrap_protocol::config::StorageConfig::Plain {
                    dir: source_record.storage.dir().clone(),
                }
            }
            underwrap_protocol::service::AdminStoreBackendKind::Encrypted => {
                underwrap_protocol::config::StorageConfig::Encrypted {
                    dir: source_record.storage.dir().clone(),
                }
            }
        };
        let source_generation = entry.generation();
        let leases = Arc::clone(&entry.leases);
        entry.state = StoreState::Busy;
        (
            source_record,
            target_record,
            source_backend,
            source_generation,
            leases,
        )
    };

    let mut cancellation = BusyCancellationGuard {
        state: Arc::clone(&state),
        store_id: name.clone(),
        identity: Arc::clone(&identity),
        lifecycle_lock: Arc::clone(&lifecycle_lock),
        source_generation,
        source_backend: Some(Arc::clone(&source_backend)),
        source_kind,
        active: true,
    };
    if !leases
        .wait_for_zero(std::time::Duration::from_secs(30))
        .await
    {
        log_admin_event(
            "store.migrate",
            &principal,
            AuditEntity::Store { store: &name },
            Some("lease-timeout"),
            "failure",
        );
        return Err(AdminError::store_busy());
    }
    cancellation.disarm();

    let stores_dir = state.admin().stores_dir.clone();
    let migration = migration::migrate_generation(
        &name,
        source_record.storage.dir(),
        source_backend.as_ref(),
        source_kind,
        body.backend,
        target_key.as_ref(),
        hooks,
        || {
            let _guard = state.admin().store_collection_lock.lock().unwrap();
            crate::store_registry::save_store_record(&stores_dir, &target_record)
        },
        || {
            let _guard = state.admin().store_collection_lock.lock().unwrap();
            crate::store_registry::save_store_record(&stores_dir, &source_record)
        },
    );
    let migrated = match migration {
        Ok(success) => success,
        Err(failure) => {
            let mut registry = state.shared.stores.write().unwrap();
            let publishable = registry.entries.get_mut(&name).filter(|entry| {
                Arc::ptr_eq(&entry.identity, &identity)
                    && Arc::ptr_eq(&entry.lifecycle_lock, &lifecycle_lock)
                    && entry.generation() == source_generation
                    && matches!(entry.state, StoreState::Busy)
            });
            if let Some(entry) = publishable {
                if failure.target_record_persisted {
                    entry.record = target_record.clone();
                }
                entry.state = match failure.disposition {
                    migration::FailureDisposition::RestoreSource => match source_kind {
                        underwrap_protocol::service::AdminStoreBackendKind::Plain => {
                            StoreState::Ready(Arc::clone(&source_backend))
                        }
                        underwrap_protocol::service::AdminStoreBackendKind::Encrypted => {
                            StoreState::Unlocked(Arc::clone(&source_backend))
                        }
                    },
                    migration::FailureDisposition::RecoveryRequired => StoreState::RecoveryRequired,
                };
                entry.advance_generation();
            } else {
                return Err(AdminError::migration_recovery_required());
            }
            drop(registry);
            log_admin_event(
                "store.migrate",
                &principal,
                AuditEntity::Store { store: &name },
                Some(failure.audit_reason),
                "failure",
            );
            return Err(failure.error);
        }
    };

    {
        let mut registry = state.shared.stores.write().unwrap();
        let Some(entry) = registry.entries.get_mut(&name) else {
            return Err(AdminError::migration_recovery_required());
        };
        if !Arc::ptr_eq(&entry.identity, &identity)
            || !Arc::ptr_eq(&entry.lifecycle_lock, &lifecycle_lock)
            || entry.generation() != source_generation
            || !matches!(entry.state, StoreState::Busy)
        {
            return Err(AdminError::migration_recovery_required());
        }
        entry.record = target_record.clone();
        entry.state = match body.backend {
            underwrap_protocol::service::AdminStoreBackendKind::Plain => {
                StoreState::Ready(Arc::clone(&migrated.backend))
            }
            underwrap_protocol::service::AdminStoreBackendKind::Encrypted => {
                StoreState::Unlocked(Arc::clone(&migrated.backend))
            }
        };
        entry.advance_generation();
    }
    let usage = underwrap_protocol::service::AdminStoreUsageSummary {
        mapped_client_instance_count: super::mapped_client_instance_count(&state, &name),
        has_secrets: migrated.has_secrets,
    };
    let lifecycle = match body.backend {
        underwrap_protocol::service::AdminStoreBackendKind::Plain => {
            underwrap_protocol::service::AdminStoreLifecycleState::Ready
        }
        underwrap_protocol::service::AdminStoreBackendKind::Encrypted => {
            underwrap_protocol::service::AdminStoreLifecycleState::Unlocked
        }
    };
    let actions = super::admin_store_actions(lifecycle, &usage);
    log_admin_event(
        "store.migrate",
        &principal,
        AuditEntity::Store { store: &name },
        Some(match body.backend {
            underwrap_protocol::service::AdminStoreBackendKind::Plain => "encrypted-to-plain",
            underwrap_protocol::service::AdminStoreBackendKind::Encrypted => "plain-to-encrypted",
        }),
        "success",
    );
    Ok(Json(AdminStoreMigrateResponse {
        store: super::admin_store_list_item(&target_record, lifecycle, usage, actions),
    }))
}

/// `POST /v1/admin/stores/{id}/rotate-key` — mint a fresh in-memory
/// master key, re-encrypt the complete store, atomically activate it, and
/// return the key once. Every displaced ciphertext generation remains in a
/// uniquely named backend-host recovery workspace until trusted manual cleanup.
/// HTTP delivery and the browser's local acknowledgement do not prove custody.
#[utoipa::path(
    post,
    path = "/v1/admin/stores/{id}/rotate-key",
    tag = "stores",
    security(("session_cookie" = [], "csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    responses(
        (status = 200, description = "Key rotated and returned once; the displaced encrypted generation is retained for backend-host recovery.", body = AdminStoreRotateKeyResponse),
        (status = 404, description = "Store not found (`not_found`).", body = ServiceErrorResponse),
        (status = 409, description = "Store is not encrypted (`not_encrypted`).", body = ServiceErrorResponse),
        (status = 500, description = "Rotation failed (`rotate_stage_failed`, `rotate_list_failed`, `rotate_canary_failed`, `rotate_read_failed`, `rotate_write_failed`, `rotate_verify_failed`, `rotate_swap_failed`, `rotate_reopen_failed`, `rotation_recovery_required`, or `migration_recovery_required`).", body = ServiceErrorResponse),
        (status = 503, description = "Store is unavailable (`store_locked` or `store_busy`).", body = ServiceErrorResponse),
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
    lifecycle_lock: Arc<tokio::sync::Mutex<()>>,
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
    Arc::ptr_eq(&entry.lifecycle_lock, &admission.lifecycle_lock)
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
    let lifecycle_lock = {
        let registry = state.shared.stores.read().unwrap();
        Arc::clone(
            &registry
                .entry(&name)
                .map_err(|_| AdminError::store_not_found())?
                .lifecycle_lock,
        )
    };
    hooks.before_rotation_lock();
    let _lifecycle_guard = lifecycle_lock.lock().await;

    let admission = {
        let registry = state.shared.stores.read().unwrap();
        let Ok(entry) = registry.entry(&name) else {
            audit_rotation_publish_failure(&principal, &name);
            return Err(AdminError::rotation_recovery_required());
        };
        if !Arc::ptr_eq(&entry.lifecycle_lock, &lifecycle_lock) {
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
            StoreState::Busy => return Err(AdminError::store_busy()),
            StoreState::RecoveryRequired => {
                return Err(AdminError::migration_recovery_required());
            }
            StoreState::Ready(_) => {
                return Err(AdminError::conflict(
                    "not_encrypted",
                    "store is not encrypted; rotate-key only applies to encrypted backends",
                ));
            }
        };
        RotationAdmission {
            lifecycle_lock: Arc::clone(&lifecycle_lock),
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
                entry.advance_generation();
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
    security(("session_cookie" = [], "csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    request_body = AdminStoreUnlockRequest,
    responses(
        (status = 200, description = "Store unlocked.", body = AdminOkResponse),
        (status = 400, description = "Key hex is invalid (`invalid_key_hex`) or the store is not encrypted (`store_not_encrypted`).", body = ServiceErrorResponse),
        (status = 403, description = "Key does not match the encrypted store (`key_mismatch`).", body = ServiceErrorResponse),
        (status = 404, description = "Store not found (`not_found`).", body = ServiceErrorResponse),
        (status = 409, description = "Store is already unlocked (`already_unlocked`).", body = ServiceErrorResponse),
        (status = 500, description = "Opening the store failed (`open_store_failed`), or trusted-host rotation or migration recovery is required (`rotation_recovery_required` or `migration_recovery_required`).", body = ServiceErrorResponse),
        (status = 503, description = "Store is busy (`store_busy`).", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_unlock(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
    Body(body): Body<AdminStoreUnlockRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let _collection_guard = state.admin().store_collection_lock.lock().unwrap();
    let (dir, identity, lifecycle_lock, generation) = {
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
            StoreState::Busy => return Err(AdminError::store_busy()),
            StoreState::RecoveryRequired => {
                return Err(AdminError::migration_recovery_required());
            }
            StoreState::Ready(_) => {
                return Err(AdminError::bad_request(
                    "store_not_encrypted",
                    "plain stores are never locked",
                ));
            }
        }
        (
            dir,
            Arc::clone(&entry.identity),
            Arc::clone(&entry.lifecycle_lock),
            entry.generation(),
        )
    };
    {
        let registry = state.shared.stores.read().unwrap();
        let entry = registry
            .entry(&name)
            .map_err(|_| AdminError::store_not_found())?;
        if !Arc::ptr_eq(&entry.identity, &identity)
            || !Arc::ptr_eq(&entry.lifecycle_lock, &lifecycle_lock)
            || entry.generation() != generation
        {
            return Err(AdminError::store_busy());
        }
        match entry.state {
            StoreState::Locked => {}
            StoreState::Busy => return Err(AdminError::store_busy()),
            StoreState::RecoveryRequired => {
                return Err(AdminError::migration_recovery_required());
            }
            StoreState::Ready(_) | StoreState::Unlocked(_) => {
                return Err(AdminError::conflict(
                    "already_unlocked",
                    "store is already unlocked",
                ));
            }
        }
    }
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
        let entry = reg
            .entries
            .get(&name)
            .ok_or_else(AdminError::store_not_found)?;
        if !Arc::ptr_eq(&entry.identity, &identity)
            || !Arc::ptr_eq(&entry.lifecycle_lock, &lifecycle_lock)
            || entry.generation() != generation
        {
            return Err(AdminError::store_busy());
        }
        match entry.state {
            StoreState::Locked => reg.unlock(&name, Arc::new(backend)),
            StoreState::Busy => return Err(AdminError::store_busy()),
            StoreState::RecoveryRequired => {
                return Err(AdminError::migration_recovery_required());
            }
            StoreState::Ready(_) | StoreState::Unlocked(_) => {
                return Err(AdminError::conflict(
                    "already_unlocked",
                    "store is already unlocked",
                ));
            }
        }
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
    security(("session_cookie" = [], "csrf_header" = [])),
    params(("id" = String, Path, description = "Store id.")),
    responses(
        (status = 200, description = "Store locked; in-memory key material wiped.", body = AdminStoreLockResponse),
        (status = 400, description = "Store is not encrypted (`store_not_encrypted`).", body = ServiceErrorResponse),
        (status = 404, description = "Store not found (`not_found`).", body = ServiceErrorResponse),
        (status = 500, description = "Trusted-host migration recovery is required (`migration_recovery_required`).", body = ServiceErrorResponse),
        (status = 503, description = "Store is busy (`store_busy`).", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_store_lock(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<AdminStoreLockResponse>, AdminError> {
    let _collection_guard = state.admin().store_collection_lock.lock().unwrap();
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
    let was_locked = match entry.state {
        StoreState::Locked => true,
        StoreState::Unlocked(_) => false,
        StoreState::Busy => return Err(AdminError::store_busy()),
        StoreState::RecoveryRequired => {
            return Err(AdminError::migration_recovery_required());
        }
        StoreState::Ready(_) => {
            return Err(AdminError::bad_request(
                "store_not_encrypted",
                "plain stores are never locked",
            ));
        }
    };
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
#[path = "lifecycle/migration_handler_tests.rs"]
mod migration_handler_tests;
#[cfg(test)]
mod tests;
