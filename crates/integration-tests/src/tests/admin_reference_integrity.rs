//! Real authenticated admin-HTTPS coverage for selected-store reference gates.

use anyhow::{Context, Result, ensure};
use base64::Engine;
use reqwest::{Method, StatusCode};
use underwrap_protocol::policy::WrapperApprovalMode;
use underwrap_protocol::service::{
    AdminLibsecretCollectionCreateRequest, AdminPairCreateRequest, AdminSecretWriteRequest,
    AdminWrapperPermissionUpsertRequest,
};

use super::*;

const MISSING_RESOURCE: &str = "res_missing_reference";

async fn get_json(daemon: &TestDaemon, path: &str) -> Result<serde_json::Value> {
    let response = daemon.admin_request(Method::GET, path).send().await?;
    ensure!(
        response.status().is_success(),
        "GET {path} returned {}",
        response.status()
    );
    Ok(response.json().await?)
}

async fn current_views(daemon: &TestDaemon) -> Result<Vec<serde_json::Value>> {
    let paths = [
        "/v1/admin/stores/test/secrets",
        "/v1/admin/stores/test/resources",
        "/v1/admin/stores/test/pairs",
        "/v1/admin/stores/test/wrapper-permissions",
        "/v1/admin/client-instances",
    ];
    let mut views = Vec::with_capacity(paths.len());
    for path in paths {
        views.push(get_json(daemon, path).await?);
    }
    Ok(views)
}

async fn expect_reference_error(response: reqwest::Response, expected_message: &str) -> Result<()> {
    ensure!(
        response.status() == StatusCode::NOT_FOUND,
        "expected 404, got {}",
        response.status()
    );
    let body: serde_json::Value = response.json().await?;
    ensure!(
        body == serde_json::json!({
            "code": "not_found",
            "message": expected_message,
        }),
        "unexpected error envelope: {body}"
    );
    Ok(())
}

async fn run_matrix(daemon: &TestDaemon) -> Result<()> {
    let before = current_views(daemon).await?;

    let flat = daemon
        .admin_request(
            Method::PUT,
            &format!("/v1/admin/stores/test/secrets/{MISSING_RESOURCE}/env/TEST_TOKEN"),
        )
        .json(&AdminSecretWriteRequest {
            material_b64: base64::engine::general_purpose::STANDARD.encode(b"synthetic-flat"),
        })
        .send()
        .await?;
    expect_reference_error(flat, "unknown resource").await?;

    let libsecret = daemon
        .admin_request(
            Method::POST,
            &format!("/v1/admin/stores/test/resources/{MISSING_RESOURCE}/libsecret/collections"),
        )
        .json(&AdminLibsecretCollectionCreateRequest {
            id: Some("default".into()),
            label: "Default".into(),
        })
        .send()
        .await?;
    expect_reference_error(libsecret, "unknown resource").await?;

    let permission = daemon
        .admin_request(
            Method::PUT,
            &format!(
                "/v1/admin/stores/test/wrapper-permissions/missing-client/gh-authorized/{MISSING_RESOURCE}"
            ),
        )
        .json(&AdminWrapperPermissionUpsertRequest {
            mode: WrapperApprovalMode::Unlocked,
            writable: false,
            reason: None,
        })
        .send()
        .await?;
    expect_reference_error(permission, "unknown client instance").await?;

    let pair = daemon
        .admin_request(Method::POST, "/v1/admin/stores/test/pairs")
        .json(&AdminPairCreateRequest {
            wrapper: "gh-authorized".into(),
            resource_id: MISSING_RESOURCE.into(),
        })
        .send()
        .await?;
    expect_reference_error(pair, "unknown resource").await?;

    let after = current_views(daemon).await?;
    ensure!(
        after == before,
        "one or more rejected requests changed admin views"
    );
    ensure!(
        !daemon
            .store_dir_for("test")
            .join("resources")
            .join(MISSING_RESOURCE)
            .exists(),
        "a rejected request created an orphan resource blob"
    );

    // Prove the same authenticated session and live service still accept the
    // corresponding valid operations.
    let resource = daemon
        .create_resource("test", "valid/reference")
        .await?
        .resource;
    daemon
        .create_pair("test", "gh-authorized", &resource.id)
        .await?;
    daemon
        .admin_write_raw("test", "gh-authorized", &resource.id, "synthetic-valid")
        .await?;

    let collection = daemon
        .admin_request(
            Method::POST,
            &format!(
                "/v1/admin/stores/test/resources/{}/libsecret/collections",
                resource.id
            ),
        )
        .json(&AdminLibsecretCollectionCreateRequest {
            id: Some("default".into()),
            label: "Default".into(),
        })
        .send()
        .await?;
    ensure!(
        collection.status().is_success(),
        "valid libsecret request returned {}",
        collection.status()
    );

    let mut valid_permission = daemon.current_permission("gh-authorized", &resource.id);
    valid_permission.writable = true;
    daemon
        .upsert_permission("test", &valid_permission)
        .await
        .context("valid permission upsert")?;

    let blob = daemon.get_resource_blob("test", &resource.id)?;
    ensure!(blob.env.contains_key("TEST_TOKEN"));
    ensure!(blob.libsecret.collections.contains_key("default"));
    let policy = daemon.get_policy("test").await?;
    ensure!(policy.permissions.iter().any(|candidate| {
        candidate.client_instance == daemon.client_instance()
            && candidate.wrapper == "gh-authorized"
            && candidate.resource_id == resource.id
            && candidate.writable
    }));
    Ok(())
}

#[tokio::test]
async fn admin_reference_integrity_real_authenticated_https_matrix() {
    let daemon = TestDaemon::start(one_wrapper_config()).await.unwrap();
    let result = run_matrix(&daemon).await;
    let stop_result = daemon.stop().await;
    result.unwrap();
    stop_result.unwrap();
}
