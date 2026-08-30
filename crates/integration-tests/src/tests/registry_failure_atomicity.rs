//! Real authenticated admin-HTTPS/runtime-mTLS registry failure evidence.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use reqwest::{Method, StatusCode};
use underwrap_backend::runtime_clients::{ClientInstanceRecord, ClientInstanceStatus};
use underwrap_protocol::config::StoreRecord;
use underwrap_protocol::service::{
    AdminClientInstanceListItem, AdminClientInstanceStatus, AdminClientInstanceUpdateRequest,
    AdminStoreBackendKind, AdminStoreCreateRequest, AdminStoreDeleteResponse, AdminStoreListItem,
    AdminStoreUpdateRequest, AdminStoresListResponse, ServiceErrorResponse,
};

use super::*;

/// Make a configured registry path fail as a directory while preserving its
/// seeded contents for deterministic restoration. Drop is a final best-effort
/// guard; normal assertions explicitly restore before inspecting live/disk state.
struct RegistryDirBlocker {
    path: PathBuf,
    saved_path: PathBuf,
    restored: bool,
}

impl RegistryDirBlocker {
    fn block(path: &Path) -> Result<Self> {
        let saved_path = path.with_extension("registry-failure-saved");
        ensure!(
            !saved_path.exists(),
            "registry blocker save path already exists"
        );
        std::fs::rename(path, &saved_path).context("moving registry directory aside")?;
        let blocker = Self {
            path: path.to_path_buf(),
            saved_path,
            restored: false,
        };
        if let Err(error) = std::fs::write(path, b"synthetic registry blocker") {
            drop(blocker);
            return Err(error).context("installing registry directory blocker");
        }
        Ok(blocker)
    }

    fn restore(&mut self) -> Result<()> {
        if self.restored {
            return Ok(());
        }
        std::fs::remove_file(&self.path).context("removing registry directory blocker")?;
        std::fs::rename(&self.saved_path, &self.path).context("restoring registry directory")?;
        self.restored = true;
        Ok(())
    }
}

impl Drop for RegistryDirBlocker {
    fn drop(&mut self) {
        if !self.restored {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::rename(&self.saved_path, &self.path);
        }
    }
}

async fn expect_runtime_success(response: reqwest::Response) -> Result<()> {
    ensure!(
        response.status().is_success(),
        "runtime registration returned {}",
        response.status()
    );
    Ok(())
}

async fn expect_runtime_rejection(response: reqwest::Response) -> Result<()> {
    ensure!(
        response.status() == StatusCode::UNAUTHORIZED,
        "expected runtime rejection HTTP 401, got {}",
        response.status()
    );
    let body: serde_json::Value = response.json().await?;
    let object = body
        .as_object()
        .context("runtime rejection body was not a JSON object")?;
    ensure!(
        object.len() == 2 && object.contains_key("code") && object.contains_key("message"),
        "runtime rejection body was not the complete canonical error envelope"
    );
    let envelope: ServiceErrorResponse = serde_json::from_value(body)?;
    ensure!(
        envelope.code == "unauthorized",
        "unexpected runtime rejection code: {}",
        envelope.code
    );
    Ok(())
}

async fn expect_registry_error(
    response: reqwest::Response,
    expected_code: &str,
    expected_message: &str,
    blocked_path: &Path,
) -> Result<()> {
    ensure!(
        response.status() == StatusCode::INTERNAL_SERVER_ERROR,
        "expected registry failure HTTP 500, got {}",
        response.status()
    );
    let body: serde_json::Value = response.json().await?;
    let object = body
        .as_object()
        .context("registry failure body was not a JSON object")?;
    ensure!(
        object.len() == 2 && object.contains_key("code") && object.contains_key("message"),
        "registry failure body was not the complete canonical error envelope"
    );
    let envelope: ServiceErrorResponse = serde_json::from_value(body)?;
    ensure!(
        envelope.code == expected_code,
        "unexpected registry failure code: {}",
        envelope.code
    );
    ensure!(
        envelope.message == expected_message,
        "unexpected registry failure message: {}",
        envelope.message
    );
    ensure!(
        !envelope
            .message
            .contains(blocked_path.to_string_lossy().as_ref()),
        "registry failure response exposed the blocked registry path"
    );
    Ok(())
}

async fn list_stores(daemon: &TestDaemon) -> Result<AdminStoresListResponse> {
    let response = daemon
        .admin_request(Method::GET, "/v1/admin/stores")
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "store list returned {}",
        response.status()
    );
    Ok(response.json().await?)
}

async fn live_store(daemon: &TestDaemon, id: &str) -> Result<AdminStoreListItem> {
    list_stores(daemon)
        .await?
        .items
        .into_iter()
        .find(|item| item.id == id)
        .with_context(|| format!("store {id} missing from live list"))
}

async fn live_client(daemon: &TestDaemon, id: &str) -> Result<AdminClientInstanceListItem> {
    daemon
        .list_client_instances()
        .await?
        .items
        .into_iter()
        .find(|item| item.id == id)
        .with_context(|| format!("runtime client instance {id} missing from live list"))
}

fn disk_store(daemon: &TestDaemon, id: &str) -> Result<StoreRecord> {
    underwrap_backend::store_registry::load_store_records(&daemon.stores_dir)?
        .remove(id)
        .with_context(|| format!("store {id} missing from fresh registry load"))
}

fn disk_client(daemon: &TestDaemon, id: &str) -> Result<ClientInstanceRecord> {
    underwrap_backend::runtime_clients::load_client_instances(&daemon.clients_dir)?
        .remove(id)
        .with_context(|| format!("runtime client instance {id} missing from fresh registry load"))
}

async fn run_matrix(daemon: &TestDaemon) -> Result<()> {
    let replacement_store = daemon
        .create_store(&AdminStoreCreateRequest {
            name: "replacement store".into(),
            backend: AdminStoreBackendKind::Plain,
            description: Some("runtime remap target".into()),
        })
        .await?
        .store;
    let delete_store = daemon
        .create_store(&AdminStoreCreateRequest {
            name: "delete candidate".into(),
            backend: AdminStoreBackendKind::Plain,
            description: Some("registry deletion evidence".into()),
        })
        .await?
        .store;

    // Store update: the failed save changes neither the authenticated list view
    // nor a fresh load. The same request then succeeds without a restart.
    let store_before_live = live_store(daemon, "test").await?;
    let store_before_disk = disk_store(daemon, "test")?;
    let store_update = AdminStoreUpdateRequest {
        name: Some("updated test store".into()),
        description: Some("persisted replacement metadata".into()),
    };
    let mut blocker = RegistryDirBlocker::block(&daemon.stores_dir)?;
    let response = daemon
        .admin_request(Method::PUT, "/v1/admin/stores/test")
        .json(&store_update)
        .send()
        .await?;
    expect_registry_error(
        response,
        "write_store_registry_failed",
        "could not persist store registry",
        &daemon.stores_dir,
    )
    .await?;
    blocker.restore()?;
    ensure!(live_store(daemon, "test").await? == store_before_live);
    ensure!(disk_store(daemon, "test")? == store_before_disk);

    let response = daemon
        .admin_request(Method::PUT, "/v1/admin/stores/test")
        .json(&store_update)
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "store update retry returned {}",
        response.status()
    );
    let updated_live: AdminStoreListItem = response.json().await?;
    ensure!(updated_live == live_store(daemon, "test").await?);
    ensure!(updated_live.name == "updated test store");
    ensure!(updated_live.description.as_deref() == Some("persisted replacement metadata"));
    let mut expected_store = store_before_disk;
    expected_store.name = "updated test store".into();
    expected_store.description = Some("persisted replacement metadata".into());
    ensure!(disk_store(daemon, "test")? == expected_store);

    // Store delete: the failed unlink leaves the complete entry available in
    // both views; retry removes it from both.
    let delete_before_live = live_store(daemon, &delete_store.id).await?;
    let delete_before_disk = disk_store(daemon, &delete_store.id)?;
    let delete_path = format!("/v1/admin/stores/{}", delete_store.id);
    let mut blocker = RegistryDirBlocker::block(&daemon.stores_dir)?;
    let response = daemon
        .admin_request(Method::DELETE, &delete_path)
        .send()
        .await?;
    expect_registry_error(
        response,
        "write_store_registry_failed",
        "could not persist store registry",
        &daemon.stores_dir,
    )
    .await?;
    blocker.restore()?;
    ensure!(live_store(daemon, &delete_store.id).await? == delete_before_live);
    ensure!(disk_store(daemon, &delete_store.id)? == delete_before_disk);

    let response = daemon
        .admin_request(Method::DELETE, &delete_path)
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "store delete retry returned {}",
        response.status()
    );
    let deleted: AdminStoreDeleteResponse = response.json().await?;
    ensure!(deleted.ok && deleted.cleanup_hint.is_none());
    ensure!(
        list_stores(daemon)
            .await?
            .items
            .iter()
            .all(|item| item.id != delete_store.id)
    );
    ensure!(
        !underwrap_backend::store_registry::load_store_records(&daemon.stores_dir)?
            .contains_key(&delete_store.id)
    );

    // Client update: one failed request proposes every authorization-relevant
    // field at once. Neither identity can observe a partially published record.
    let client_id = daemon.client_instance().to_string();
    let client_before_live = live_client(daemon, &client_id).await?;
    let client_before_disk = disk_client(daemon, &client_id)?;
    let failed_client_update = AdminClientInstanceUpdateRequest {
        display_name: Some("replacement runtime client".into()),
        current_store_id: Some(replacement_store.id.clone()),
        status: Some(AdminClientInstanceStatus::Disabled),
        cert_pem: Some(TEST_CLIENT_TUNNEL_PEM.into()),
        auto_create_wrapper_assets: Some(true),
    };
    let client_path = format!("/v1/admin/client-instances/{client_id}");
    let mut blocker = RegistryDirBlocker::block(&daemon.clients_dir)?;
    let response = daemon
        .admin_request(Method::PUT, &client_path)
        .json(&failed_client_update)
        .send()
        .await?;
    expect_registry_error(
        response,
        "write_client_instances_failed",
        "could not persist client instance registry",
        &daemon.clients_dir,
    )
    .await?;
    blocker.restore()?;
    ensure!(live_client(daemon, &client_id).await? == client_before_live);
    ensure!(disk_client(daemon, &client_id)? == client_before_disk);
    expect_runtime_success(
        daemon
            .direct_service_status_as(TestRuntimeIdentity::Local)
            .await?,
    )
    .await?;
    expect_runtime_rejection(
        daemon
            .direct_service_status_as(TestRuntimeIdentity::Tunnel)
            .await?,
    )
    .await?;

    // Retry the replacement as Active so its certificate can prove the new
    // authority. Build the exact expected disk record after the successful
    // local probe, which may legitimately advance last_seen_at.
    let mut expected_client = disk_client(daemon, &client_id)?;
    expected_client.display_name = "replacement runtime client".into();
    expected_client.current_store_id = replacement_store.id.clone();
    expected_client.status = ClientInstanceStatus::Active;
    expected_client.current_credential =
        underwrap_backend::runtime_clients::current_credential_from_pem(
            TEST_CLIENT_TUNNEL_PEM.into(),
        )?;
    expected_client.auto_create_wrapper_assets = true;
    let successful_client_update = AdminClientInstanceUpdateRequest {
        status: Some(AdminClientInstanceStatus::Active),
        ..failed_client_update
    };
    let updated_client = daemon
        .update_client_instance(&client_id, &successful_client_update)
        .await?;
    ensure!(updated_client == live_client(daemon, &client_id).await?);
    ensure!(updated_client.display_name == "replacement runtime client");
    ensure!(updated_client.current_store_id == replacement_store.id);
    ensure!(updated_client.current_store_name == replacement_store.name);
    ensure!(updated_client.status == AdminClientInstanceStatus::Active);
    ensure!(updated_client.last_seen_at == expected_client.last_seen_at);
    ensure!(
        updated_client.current_credential_fingerprint
            == expected_client.current_credential.cert_fingerprint
    );
    ensure!(updated_client.auto_create_wrapper_assets);
    ensure!(disk_client(daemon, &client_id)? == expected_client);
    expect_runtime_success(
        daemon
            .direct_service_status_as(TestRuntimeIdentity::Tunnel)
            .await?,
    )
    .await?;
    expect_runtime_rejection(
        daemon
            .direct_service_status_as(TestRuntimeIdentity::Local)
            .await?,
    )
    .await?;

    // Status is controlled by authenticated admin HTTPS. A disabled client is
    // not expected to authenticate itself in order to become active again.
    let disabled = daemon
        .update_client_instance(
            &client_id,
            &AdminClientInstanceUpdateRequest {
                display_name: None,
                current_store_id: None,
                status: Some(AdminClientInstanceStatus::Disabled),
                cert_pem: None,
                auto_create_wrapper_assets: None,
            },
        )
        .await?;
    ensure!(disabled.status == AdminClientInstanceStatus::Disabled);
    expect_runtime_rejection(
        daemon
            .direct_service_status_as(TestRuntimeIdentity::Tunnel)
            .await?,
    )
    .await?;
    let enabled = daemon
        .update_client_instance(
            &client_id,
            &AdminClientInstanceUpdateRequest {
                display_name: None,
                current_store_id: None,
                status: Some(AdminClientInstanceStatus::Active),
                cert_pem: None,
                auto_create_wrapper_assets: None,
            },
        )
        .await?;
    ensure!(enabled.status == AdminClientInstanceStatus::Active);
    expect_runtime_success(
        daemon
            .direct_service_status_as(TestRuntimeIdentity::Tunnel)
            .await?,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn registry_failure_atomicity_real_authenticated_https_mtls_matrix() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let result = run_matrix(&daemon).await;
    let stop_result = daemon.stop().await;
    result.unwrap();
    stop_result.unwrap();
}
