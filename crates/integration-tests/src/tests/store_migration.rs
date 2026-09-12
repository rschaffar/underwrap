//! Real authenticated admin-HTTPS and filesystem coverage for store migration.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use underwrap_backend_store::{EncryptedBackend, PlainBackend, StorageBackend};
use underwrap_protocol::entities::{
    PAIRS_DOCUMENT_SECRET_ID, RESOURCES_DOCUMENT_SECRET_ID, ResourceRecord,
    WrapperResourcePairRecord,
};
use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::policy::{POLICY_DOCUMENT_SECRET_ID, PolicyDocument};
use underwrap_protocol::service::{
    AdminKeyHex, AdminStoreActionState, AdminStoreBackendKind, AdminStoreCreateRequest,
    AdminStoreCreateResponse, AdminStoreKeyResponse, AdminStoreLifecycleState, AdminStoreListItem,
    AdminStoreLockResponse, AdminStoreMigrateRequest, AdminStoreMigrateResponse,
    AdminStoreUnlockRequest, AdminStoreUsageSummary, AdminStoresListResponse,
    AdminWrapperPermissionUpsertRequest, ServiceErrorResponse,
};

use super::*;

const STORE_NAME: &str = "migration integration store";
const STORE_DESCRIPTION: &str = "metadata retained across backend migration";
const RESOURCE_NAME: &str = "github/token";
const NESTED_STORAGE_ID: &str = "nested/operator/state/document";
const RESOURCE_LOGICAL_BYTES: &str = "synthetic resource migration material";
const NESTED_LOGICAL_BYTE: u8 = b'x';
const NESTED_LOGICAL_LEN: usize = 4 * 1024 * 1024;

struct AdminProjection {
    resources: Vec<ResourceRecord>,
    pairs: Vec<WrapperResourcePairRecord>,
    policy: PolicyDocument,
    mapped_clients: Vec<(String, String, String)>,
}

fn directory_entries(path: &Path) -> Result<BTreeSet<OsString>> {
    std::fs::read_dir(path)?
        .map(|entry| Ok(entry?.file_name()))
        .collect()
}

async fn list_stores(daemon: &TestDaemon) -> Result<AdminStoresListResponse> {
    let response = daemon
        .admin_request(reqwest::Method::GET, "/v1/admin/stores")
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "store list returned {}",
        response.status()
    );
    Ok(response.json().await?)
}

async fn store_item(daemon: &TestDaemon, store_id: &str) -> Result<AdminStoreListItem> {
    list_stores(daemon)
        .await?
        .items
        .into_iter()
        .find(|store| store.id == store_id)
        .with_context(|| format!("store {store_id} was absent from the admin projection"))
}

async fn admin_projection(daemon: &TestDaemon, store_id: &str) -> Result<AdminProjection> {
    let resources = daemon.list_resources(store_id).await?.resources;
    let pairs = daemon.list_pairs(store_id).await?.pairs;
    let policy = daemon.get_policy(store_id).await?;
    let mut mapped_clients = daemon
        .list_client_instances()
        .await?
        .items
        .into_iter()
        .filter(|client| client.current_store_id == store_id)
        .map(|client| {
            (
                client.id,
                client.current_store_id,
                client.current_store_name,
            )
        })
        .collect::<Vec<_>>();
    mapped_clients.sort();
    Ok(AdminProjection {
        resources,
        pairs,
        policy,
        mapped_clients,
    })
}

fn ensure_projection_preserved(
    expected: &AdminProjection,
    actual: &AdminProjection,
    direction: &str,
) -> Result<()> {
    ensure!(
        actual.resources == expected.resources,
        "resource catalog changed during {direction} migration"
    );
    ensure!(
        actual.pairs == expected.pairs,
        "wrapper/resource pairs changed during {direction} migration"
    );
    ensure!(
        actual.policy == expected.policy,
        "permission rows changed during {direction} migration"
    );
    ensure!(
        actual.mapped_clients == expected.mapped_clients,
        "runtime client mapping changed during {direction} migration"
    );
    Ok(())
}

fn storage_snapshot<B: StorageBackend>(backend: &B) -> Result<BTreeMap<String, SecretBytes>> {
    let mut snapshot = BTreeMap::new();
    for id in backend.list("")? {
        let value = backend
            .read(&id)?
            .with_context(|| format!("listed storage id {id} was unreadable"))?;
        snapshot.insert(id, value);
    }
    Ok(snapshot)
}

fn ensure_private_root(path: &Path, direction: &str) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.file_type().is_dir() && !metadata.file_type().is_symlink(),
        "{direction} target root was not a regular non-symlink directory"
    );
    ensure!(
        metadata.permissions().mode() & 0o7777 == 0o700,
        "{direction} target root mode was not exactly 0700"
    );
    Ok(())
}

fn ensure_canonical_key(key: &AdminKeyHex) -> Result<()> {
    let exposed = key.expose();
    ensure!(
        exposed.len() == 64,
        "generated key length was not canonical"
    );
    ensure!(
        exposed
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "generated key alphabet was not canonical"
    );
    Ok(())
}

async fn generate_key(daemon: &TestDaemon) -> Result<AdminStoreKeyResponse> {
    let response = daemon
        .admin_request(reqwest::Method::POST, "/v1/admin/store-keys")
        .send()
        .await?;
    ensure!(response.status().is_success(), "key generation failed");
    ensure!(
        response
            .headers()
            .get_all(reqwest::header::CACHE_CONTROL)
            .iter()
            .count()
            == 1
            && response.headers()[reqwest::header::CACHE_CONTROL]
                == "no-store, no-cache, must-revalidate",
        "key response Cache-Control header was not exact"
    );
    ensure!(
        response
            .headers()
            .get_all(reqwest::header::PRAGMA)
            .iter()
            .count()
            == 1
            && response.headers()[reqwest::header::PRAGMA] == "no-cache",
        "key response Pragma header was not exact"
    );
    ensure!(
        response
            .headers()
            .get_all(reqwest::header::EXPIRES)
            .iter()
            .count()
            == 1
            && response.headers()[reqwest::header::EXPIRES] == "0",
        "key response Expires header was not exact"
    );
    let generated: AdminStoreKeyResponse = response.json().await?;
    ensure_canonical_key(&generated.key_hex)?;
    Ok(generated)
}

async fn expect_error(
    response: reqwest::Response,
    expected_status: reqwest::StatusCode,
    expected_code: &str,
) -> Result<()> {
    ensure!(
        response.status() == expected_status,
        "expected HTTP {expected_status}, got {}",
        response.status()
    );
    let error: ServiceErrorResponse = response.json().await?;
    ensure!(error.code == expected_code, "unexpected error code");
    Ok(())
}

async fn assert_runtime_resource(daemon: &TestDaemon) -> Result<()> {
    let (_, blob, _) = checkout_resource(daemon.instance_socket.clone(), "gh-authorized").await;
    ensure!(blob.env.len() == 1, "runtime resource env shape changed");
    let value = blob
        .env
        .get("TEST_TOKEN")
        .context("runtime resource lost its env entry")?;
    ensure!(
        value.as_ref() == RESOURCE_LOGICAL_BYTES.as_bytes(),
        "runtime resource logical bytes changed"
    );
    Ok(())
}

#[tokio::test]
async fn authenticated_https_preserves_store_across_both_migration_directions() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let result = async {
        let stores_before_generation = list_stores(&daemon).await?;
        let roots_before_generation = directory_entries(&daemon.stores_data_root)?;
        let creation_key = generate_key(&daemon).await?;
        ensure!(
            list_stores(&daemon).await? == stores_before_generation,
            "key generation changed the store registry projection"
        );
        ensure!(
            directory_entries(&daemon.stores_data_root)? == roots_before_generation,
            "key generation changed the managed store data root"
        );

        let missing_key = daemon
            .admin_request(reqwest::Method::POST, "/v1/admin/stores")
            .json(&AdminStoreCreateRequest {
                name: "rejected encrypted candidate".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
                key_hex: None,
            })
            .send()
            .await?;
        expect_error(
            missing_key,
            reqwest::StatusCode::BAD_REQUEST,
            "key_required",
        )
        .await?;
        ensure!(
            list_stores(&daemon).await? == stores_before_generation,
            "rejected encrypted creation changed the store registry"
        );
        ensure!(
            directory_entries(&daemon.stores_data_root)? == roots_before_generation,
            "rejected encrypted creation changed the managed data root"
        );

        let nonexact_create = daemon
            .admin_request(reqwest::Method::POST, "/v1/admin/stores")
            .json(&AdminStoreCreateRequest {
                name: "rejected nonexact key candidate".into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: None,
                key_hex: Some(AdminKeyHex::new(format!(
                    " {}",
                    creation_key.key_hex.expose()
                ))),
            })
            .send()
            .await?;
        expect_error(
            nonexact_create,
            reqwest::StatusCode::BAD_REQUEST,
            "invalid_key_hex",
        )
        .await?;
        ensure!(
            list_stores(&daemon).await? == stores_before_generation,
            "nonexact encrypted creation changed the store registry"
        );
        ensure!(
            directory_entries(&daemon.stores_data_root)? == roots_before_generation,
            "nonexact encrypted creation changed the managed data root"
        );

        let create = daemon
            .admin_request(reqwest::Method::POST, "/v1/admin/stores")
            .json(&AdminStoreCreateRequest {
                name: STORE_NAME.into(),
                backend: AdminStoreBackendKind::Encrypted,
                description: Some(STORE_DESCRIPTION.into()),
                key_hex: Some(AdminKeyHex::new(creation_key.key_hex.expose().to_owned())),
            })
            .send()
            .await?;
        ensure!(create.status().is_success(), "encrypted creation failed");
        let create_body: serde_json::Value = create.json().await?;
        ensure!(
            create_body
                .as_object()
                .is_some_and(|object| object.len() == 1)
                && create_body.get("store").is_some()
                && create_body.get("key_hex").is_none(),
            "encrypted creation returned an unexpected response shape"
        );
        let created: AdminStoreCreateResponse = serde_json::from_value(create_body)?;
        let store_id = created.store.id.clone();
        let canonical = daemon.store_dir_for(&store_id);
        let stable_created_at = created
            .store
            .created_at
            .context("created store omitted created_at")?;
        let empty_usage = AdminStoreUsageSummary {
            mapped_client_instance_count: 0,
            has_secrets: false,
        };
        let empty_unlocked_actions = AdminStoreActionState {
            can_update: true,
            can_migrate: true,
            can_rotate_key: true,
            can_unlock: false,
            can_lock: true,
            can_delete_now: true,
            can_force_delete: false,
        };
        ensure!(
            created.store.name == STORE_NAME,
            "create changed store name"
        );
        ensure!(
            created.store.description.as_deref() == Some(STORE_DESCRIPTION),
            "create changed store description"
        );
        ensure!(
            created.store.backend == AdminStoreBackendKind::Encrypted
                && created.store.lifecycle_state == AdminStoreLifecycleState::Unlocked
                && created.store.usage == empty_usage
                && created.store.actions == empty_unlocked_actions,
            "created encrypted store projection was inconsistent"
        );
        ensure_private_root(&canonical, "encrypted creation")?;

        let resource = daemon
            .create_resource(&store_id, RESOURCE_NAME)
            .await?
            .resource;
        daemon
            .create_pair(&store_id, "gh-authorized", &resource.id)
            .await?;
        daemon
            .admin_write_raw(
                &store_id,
                "gh-authorized",
                &resource.id,
                RESOURCE_LOGICAL_BYTES,
            )
            .await?;

        let client_id = daemon.client_instance().to_owned();
        daemon
            .update_client_instance(
                &client_id,
                &underwrap_protocol::service::AdminClientInstanceUpdateRequest {
                    access_override: None,
                    display_name: None,
                    current_store_id: Some(store_id.clone()),
                    status: None,
                    cert_pem: None,
                    auto_create_wrapper_assets: None,
                },
            )
            .await?;
        let permission = daemon.current_permission("gh-authorized", &resource.id);
        let permission_response = daemon
            .admin_request(
                reqwest::Method::PUT,
                &format!(
                    "/v1/admin/stores/{store_id}/wrapper-permissions/{client_id}/gh-authorized/{}",
                    resource.id
                ),
            )
            .json(&AdminWrapperPermissionUpsertRequest {
                mode: permission.mode,
                writable: permission.writable,
                reason: permission.reason,
            })
            .send()
            .await?;
        ensure!(
            permission_response.status().is_success(),
            "permission creation failed"
        );

        let mut creation_key_bytes =
            underwrap_backend_store::parse_hex_key_strict(creation_key.key_hex.expose())?;
        let encrypted = EncryptedBackend::open_existing_with_key(&canonical, creation_key_bytes)?;
        creation_key_bytes.fill(0);
        let nested_logical_bytes = vec![NESTED_LOGICAL_BYTE; NESTED_LOGICAL_LEN];
        encrypted.write(NESTED_STORAGE_ID, &nested_logical_bytes)?;
        drop(nested_logical_bytes);
        let encrypted_snapshot = storage_snapshot(&encrypted)?;
        drop(encrypted);
        for id in [
            underwrap_backend_store::ENCRYPTED_CANARY_ID,
            RESOURCES_DOCUMENT_SECRET_ID,
            PAIRS_DOCUMENT_SECRET_ID,
            POLICY_DOCUMENT_SECRET_ID,
            NESTED_STORAGE_ID,
        ] {
            ensure!(
                encrypted_snapshot.contains_key(id),
                "encrypted source omitted required storage id {id}"
            );
        }
        ensure!(
            encrypted_snapshot.contains_key(&format!("resources/{}", resource.id)),
            "encrypted source omitted the representative resource blob"
        );

        let before_projection = admin_projection(&daemon, &store_id).await?;
        ensure!(
            before_projection.mapped_clients.len() == 1,
            "fixture did not map exactly one runtime client"
        );
        let before_item = store_item(&daemon, &store_id).await?;
        let available_usage = AdminStoreUsageSummary {
            mapped_client_instance_count: 1,
            has_secrets: true,
        };
        let unlocked_actions = AdminStoreActionState {
            can_update: true,
            can_migrate: true,
            can_rotate_key: true,
            can_unlock: false,
            can_lock: true,
            can_delete_now: false,
            can_force_delete: false,
        };
        ensure!(
            before_item.lifecycle_state == AdminStoreLifecycleState::Unlocked
                && before_item.created_at == Some(stable_created_at)
                && before_item.usage == available_usage
                && before_item.actions == unlocked_actions,
            "encrypted source lifecycle projection was incomplete"
        );

        let unexpected_key = daemon
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/{store_id}/migrate"),
            )
            .json(&AdminStoreMigrateRequest {
                backend: AdminStoreBackendKind::Plain,
                key_hex: Some(AdminKeyHex::new(creation_key.key_hex.expose().to_owned())),
            })
            .send()
            .await?;
        expect_error(
            unexpected_key,
            reqwest::StatusCode::BAD_REQUEST,
            "unexpected_key",
        )
        .await?;

        let lock = daemon
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/{store_id}/lock"),
            )
            .send()
            .await?;
        ensure!(lock.status().is_success(), "store lock failed");
        let lock: AdminStoreLockResponse = lock.json().await?;
        ensure!(
            lock.ok && !lock.was_locked,
            "store lock response was invalid"
        );
        let locked_item = store_item(&daemon, &store_id).await?;
        let locked_usage = AdminStoreUsageSummary {
            mapped_client_instance_count: 1,
            has_secrets: false,
        };
        let locked_actions = AdminStoreActionState {
            can_update: true,
            can_migrate: false,
            can_rotate_key: false,
            can_unlock: true,
            can_lock: false,
            can_delete_now: false,
            can_force_delete: false,
        };
        ensure!(
            locked_item.lifecycle_state == AdminStoreLifecycleState::Locked
                && locked_item.created_at == Some(stable_created_at)
                && locked_item.usage == locked_usage
                && locked_item.actions == locked_actions,
            "locked lifecycle projection was inconsistent"
        );

        let locked_migration = daemon
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/{store_id}/migrate"),
            )
            .json(&AdminStoreMigrateRequest {
                backend: AdminStoreBackendKind::Plain,
                key_hex: None,
            })
            .send()
            .await?;
        expect_error(
            locked_migration,
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            "store_locked",
        )
        .await?;

        let unlock = daemon
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/{store_id}/unlock"),
            )
            .json(&AdminStoreUnlockRequest {
                key_hex: creation_key.key_hex.expose().to_owned(),
            })
            .send()
            .await?;
        ensure!(
            unlock.status().is_success(),
            "exact submitted key did not unlock the store"
        );
        let unlocked_item = store_item(&daemon, &store_id).await?;
        ensure!(
            unlocked_item.lifecycle_state == AdminStoreLifecycleState::Unlocked
                && unlocked_item.created_at == Some(stable_created_at)
                && unlocked_item.usage == available_usage
                && unlocked_item.actions == unlocked_actions,
            "unlocked lifecycle projection was inconsistent"
        );

        let to_plain = daemon
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/{store_id}/migrate"),
            )
            .json(&AdminStoreMigrateRequest {
                backend: AdminStoreBackendKind::Plain,
                key_hex: None,
            })
            .send()
            .await?;
        ensure!(
            to_plain.status().is_success(),
            "encrypted-to-plain migration failed"
        );
        let to_plain: AdminStoreMigrateResponse = to_plain.json().await?;
        ensure!(
            to_plain.store.id == store_id,
            "encrypted-to-plain changed store id"
        );
        ensure!(
            to_plain.store.name == STORE_NAME
                && to_plain.store.description.as_deref() == Some(STORE_DESCRIPTION),
            "encrypted-to-plain changed store metadata"
        );
        let ready_actions = AdminStoreActionState {
            can_update: true,
            can_migrate: true,
            can_rotate_key: false,
            can_unlock: false,
            can_lock: false,
            can_delete_now: false,
            can_force_delete: false,
        };
        ensure!(
            to_plain.store.backend == AdminStoreBackendKind::Plain
                && to_plain.store.lifecycle_state == AdminStoreLifecycleState::Ready
                && to_plain.store.created_at == Some(stable_created_at)
                && to_plain.store.usage == available_usage
                && to_plain.store.actions == ready_actions,
            "plain target lifecycle projection was inconsistent"
        );
        let ready_item = store_item(&daemon, &store_id).await?;
        ensure!(
            ready_item.lifecycle_state == AdminStoreLifecycleState::Ready
                && ready_item.created_at == Some(stable_created_at)
                && ready_item.usage == available_usage
                && ready_item.actions == ready_actions,
            "ready lifecycle list projection was inconsistent"
        );
        ensure_private_root(&canonical, "encrypted-to-plain")?;

        let plain = PlainBackend::open_existing(&canonical)?;
        let plain_snapshot = storage_snapshot(&plain)?;
        drop(plain);
        let mut expected_plain_snapshot = encrypted_snapshot;
        ensure!(
            expected_plain_snapshot
                .remove(underwrap_backend_store::ENCRYPTED_CANARY_ID)
                .is_some(),
            "encrypted source snapshot had no canary"
        );
        ensure!(
            plain_snapshot == expected_plain_snapshot,
            "encrypted-to-plain changed storage ids or logical bytes"
        );
        ensure_projection_preserved(
            &before_projection,
            &admin_projection(&daemon, &store_id).await?,
            "encrypted-to-plain",
        )?;

        let plain_before_generation = storage_snapshot(&PlainBackend::open_existing(&canonical)?)?;
        let stores_before_migration_key = list_stores(&daemon).await?;
        let entries_before_migration_key = directory_entries(canonical.parent().unwrap())?;
        let migration_key = generate_key(&daemon).await?;
        ensure!(
            storage_snapshot(&PlainBackend::open_existing(&canonical)?)? == plain_before_generation,
            "migration-key generation changed logical storage"
        );
        ensure!(
            list_stores(&daemon).await? == stores_before_migration_key,
            "migration-key generation changed the store registry projection"
        );
        ensure!(
            directory_entries(canonical.parent().unwrap())? == entries_before_migration_key,
            "migration-key generation changed store paths"
        );

        let missing_migration_key = daemon
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/{store_id}/migrate"),
            )
            .json(&AdminStoreMigrateRequest {
                backend: AdminStoreBackendKind::Encrypted,
                key_hex: None,
            })
            .send()
            .await?;
        expect_error(
            missing_migration_key,
            reqwest::StatusCode::BAD_REQUEST,
            "key_required",
        )
        .await?;
        ensure!(
            storage_snapshot(&PlainBackend::open_existing(&canonical)?)? == plain_snapshot,
            "missing-key migration changed the plain source"
        );

        let noncanonical_migration_key = daemon
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/{store_id}/migrate"),
            )
            .json(&AdminStoreMigrateRequest {
                backend: AdminStoreBackendKind::Encrypted,
                key_hex: Some(AdminKeyHex::new("AA".repeat(32))),
            })
            .send()
            .await?;
        expect_error(
            noncanonical_migration_key,
            reqwest::StatusCode::BAD_REQUEST,
            "invalid_key_hex",
        )
        .await?;
        ensure!(
            storage_snapshot(&PlainBackend::open_existing(&canonical)?)? == plain_snapshot,
            "noncanonical-key migration changed the plain source"
        );

        let entries_before_plain_to_encrypted = directory_entries(canonical.parent().unwrap())?;
        let to_encrypted = daemon
            .admin_request(
                reqwest::Method::POST,
                &format!("/v1/admin/stores/{store_id}/migrate"),
            )
            .json(&AdminStoreMigrateRequest {
                backend: AdminStoreBackendKind::Encrypted,
                key_hex: Some(AdminKeyHex::new(migration_key.key_hex.expose().to_owned())),
            })
            .send()
            .await?;
        ensure!(
            to_encrypted.status().is_success(),
            "plain-to-encrypted migration failed"
        );
        let to_encrypted: AdminStoreMigrateResponse = to_encrypted.json().await?;
        ensure!(
            to_encrypted.store.id == store_id,
            "plain-to-encrypted changed store id"
        );
        ensure!(
            to_encrypted.store.name == STORE_NAME
                && to_encrypted.store.description.as_deref() == Some(STORE_DESCRIPTION),
            "plain-to-encrypted changed store metadata"
        );
        ensure!(
            to_encrypted.store.backend == AdminStoreBackendKind::Encrypted
                && to_encrypted.store.lifecycle_state == AdminStoreLifecycleState::Unlocked
                && to_encrypted.store.created_at == Some(stable_created_at)
                && to_encrypted.store.usage == available_usage
                && to_encrypted.store.actions == unlocked_actions,
            "encrypted target was not published unlocked with valid actions"
        );
        let unlocked_again_item = store_item(&daemon, &store_id).await?;
        ensure!(
            unlocked_again_item.lifecycle_state == AdminStoreLifecycleState::Unlocked
                && unlocked_again_item.created_at == Some(stable_created_at)
                && unlocked_again_item.usage == available_usage
                && unlocked_again_item.actions == unlocked_actions,
            "migrated unlocked lifecycle list projection was inconsistent"
        );
        ensure_private_root(&canonical, "plain-to-encrypted")?;

        let mut migration_key_bytes =
            underwrap_backend_store::parse_hex_key_strict(migration_key.key_hex.expose())?;
        let encrypted = EncryptedBackend::open_existing_with_key(&canonical, migration_key_bytes)?;
        migration_key_bytes.fill(0);
        let encrypted_again_snapshot = storage_snapshot(&encrypted)?;
        drop(encrypted);
        let mut expected_encrypted_snapshot = plain_snapshot;
        expected_encrypted_snapshot.insert(
            underwrap_backend_store::ENCRYPTED_CANARY_ID.to_owned(),
            SecretBytes::from(underwrap_backend_store::ENCRYPTED_CANARY_PLAINTEXT),
        );
        ensure!(
            encrypted_again_snapshot == expected_encrypted_snapshot,
            "plain-to-encrypted changed storage ids or logical bytes"
        );
        ensure_projection_preserved(
            &before_projection,
            &admin_projection(&daemon, &store_id).await?,
            "plain-to-encrypted",
        )?;
        assert_runtime_resource(&daemon).await?;

        ensure!(
            directory_entries(canonical.parent().unwrap())? == entries_before_plain_to_encrypted,
            "successful plain-to-encrypted migration changed the store-data parent entry set"
        );
        let physical_nested = std::fs::read(canonical.join(NESTED_STORAGE_ID))?;
        ensure!(
            physical_nested.len() != NESTED_LOGICAL_LEN
                || physical_nested
                    .iter()
                    .any(|byte| *byte != NESTED_LOGICAL_BYTE),
            "encrypted target retained nested logical bytes as plaintext"
        );
        Ok::<(), anyhow::Error>(())
    }
    .await;
    let stop_result = daemon.stop().await;
    result.unwrap();
    stop_result.unwrap();
}
