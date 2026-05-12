//! Thin wrapper around the backendd admin HTTP API.

use anyhow::{Context, Result, bail};
use base64::Engine;
use reqwest::Client;
use underwrap_protocol::policy::{PolicyDocument, PolicyGrant, PolicyRule, WrapperApprovalMode};
use underwrap_protocol::service::{
    AdminCatalogGetRequest, AdminCatalogGetResponse, AdminGrantDeleteRequest,
    AdminGrantUpsertRequest, AdminPolicyGetRequest, AdminPolicyGetResponse, AdminRuleDeleteRequest,
    AdminRuleUpsertRequest, AdminSecretDeleteRequest, AdminSecretDeleteResponse,
    AdminSecretListRequest, AdminSecretListResponse, AdminSecretReadRequest,
    AdminSecretReadResponse, AdminSecretWriteRequest, AdminStoresResponse, AdminWrapperModeRequest,
    CatalogEntry,
};

/// One backend target as seen by the UI.
pub struct BackendClient {
    admin_url: String,
    token: String,
    http: Client,
}

impl BackendClient {
    pub fn new(admin_url: &str, token: &str, http: Client) -> Self {
        Self {
            admin_url: admin_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
            http,
        }
    }

    async fn post_json<T, R>(&self, path: &str, body: &T) -> Result<R>
    where
        T: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        let response = self
            .http
            .post(format!("{}{}", self.admin_url, path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .with_context(|| format!("sending POST {path}"))?;
        self.decode(path, response).await
    }

    async fn get_json<R>(&self, path: &str) -> Result<R>
    where
        R: serde::de::DeserializeOwned,
    {
        let response = self
            .http
            .get(format!("{}{}", self.admin_url, path))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("sending GET {path}"))?;
        self.decode(path, response).await
    }

    async fn decode<R>(&self, path: &str, response: reqwest::Response) -> Result<R>
    where
        R: serde::de::DeserializeOwned,
    {
        let status = response.status();
        let body = response
            .text()
            .await
            .with_context(|| format!("reading response body for {path}"))?;
        if !status.is_success() {
            bail!("{path} returned {status}: {body}");
        }
        serde_json::from_str(&body).with_context(|| format!("decoding response from {path}"))
    }

    pub async fn list_stores(&self) -> Result<Vec<String>> {
        let response: AdminStoresResponse = self.get_json("/v1/admin/stores").await?;
        Ok(response.stores)
    }

    pub async fn list_secrets(&self, store: &str, prefix: &str) -> Result<Vec<String>> {
        let response: AdminSecretListResponse = self
            .post_json(
                "/v1/admin/secret/list",
                &AdminSecretListRequest {
                    store: store.to_string(),
                    prefix: prefix.to_string(),
                },
            )
            .await?;
        Ok(response.ids)
    }

    pub async fn read_secret(&self, store: &str, secret_id: &str) -> Result<Option<Vec<u8>>> {
        let response: AdminSecretReadResponse = self
            .post_json(
                "/v1/admin/secret/read",
                &AdminSecretReadRequest {
                    store: store.to_string(),
                    secret_id: secret_id.to_string(),
                },
            )
            .await?;
        response
            .material_b64
            .map(|material| base64::engine::general_purpose::STANDARD.decode(material))
            .transpose()
            .context("decoding secret payload")
    }

    pub async fn delete_secret(&self, store: &str, secret_id: &str) -> Result<bool> {
        let response: AdminSecretDeleteResponse = self
            .post_json(
                "/v1/admin/secret/delete",
                &AdminSecretDeleteRequest {
                    store: store.to_string(),
                    secret_id: secret_id.to_string(),
                },
            )
            .await?;
        Ok(response.existed)
    }

    pub async fn get_catalog(&self, store: &str) -> Result<Vec<CatalogEntry>> {
        let response: AdminCatalogGetResponse = self
            .post_json(
                "/v1/admin/catalog/get",
                &AdminCatalogGetRequest {
                    store: store.to_string(),
                },
            )
            .await?;
        Ok(response.entries)
    }

    pub async fn get_policy(&self, store: &str) -> Result<PolicyDocument> {
        let response: AdminPolicyGetResponse = self
            .post_json(
                "/v1/admin/policy/get",
                &AdminPolicyGetRequest {
                    store: store.to_string(),
                },
            )
            .await?;
        Ok(response.document)
    }

    pub async fn set_wrapper_mode(
        &self,
        store: &str,
        wrapper: &str,
        mode: WrapperApprovalMode,
        reason: Option<String>,
    ) -> Result<()> {
        let _: serde_json::Value = self
            .post_json(
                "/v1/admin/wrapper/set-mode",
                &AdminWrapperModeRequest {
                    store: store.to_string(),
                    wrapper: wrapper.to_string(),
                    mode,
                    reason,
                },
            )
            .await?;
        Ok(())
    }

    pub async fn upsert_rule(&self, store: &str, rule: PolicyRule) -> Result<()> {
        let _: serde_json::Value = self
            .post_json(
                "/v1/admin/rule/upsert",
                &AdminRuleUpsertRequest {
                    store: store.to_string(),
                    rule,
                },
            )
            .await?;
        Ok(())
    }

    pub async fn delete_rule(&self, store: &str, id: &str) -> Result<()> {
        let _: serde_json::Value = self
            .post_json(
                "/v1/admin/rule/delete",
                &AdminRuleDeleteRequest {
                    store: store.to_string(),
                    id: id.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn upsert_grant(&self, store: &str, grant: PolicyGrant) -> Result<()> {
        let _: serde_json::Value = self
            .post_json(
                "/v1/admin/grant/upsert",
                &AdminGrantUpsertRequest {
                    store: store.to_string(),
                    grant,
                },
            )
            .await?;
        Ok(())
    }

    pub async fn delete_grant(&self, store: &str, id: &str) -> Result<()> {
        let _: serde_json::Value = self
            .post_json(
                "/v1/admin/grant/delete",
                &AdminGrantDeleteRequest {
                    store: store.to_string(),
                    id: id.to_string(),
                },
            )
            .await?;
        Ok(())
    }

    pub async fn write_secret(
        &self,
        store: &str,
        wrapper: &str,
        secret_id: &str,
        material: &[u8],
    ) -> Result<()> {
        let _: serde_json::Value = self
            .post_json(
                "/v1/admin/secret/write",
                &AdminSecretWriteRequest {
                    store: store.to_string(),
                    wrapper: wrapper.to_string(),
                    secret_id: secret_id.to_string(),
                    material_b64: base64::engine::general_purpose::STANDARD.encode(material),
                },
            )
            .await?;
        Ok(())
    }
}
