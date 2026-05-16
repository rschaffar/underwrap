use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::policy::{PolicyDocument, PolicyGrant, PolicyRule, WrapperApprovalMode};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceStatusResponse {
    pub ok: bool,
    pub principal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceReadRequest {
    pub secret_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceReadResponse {
    pub material_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceWriteRequest {
    pub secret_id: String,
    pub material_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceWriteResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceListRequest {
    pub prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceListResponse {
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct CatalogEntry {
    pub host: String,
    pub user: String,
    pub instance: String,
    pub wrapper: String,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceCatalogPutRequest {
    pub entries: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceCatalogPutResponse {
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceReadPrefixesRequest {
    pub prefixes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceBundleEntry {
    pub id: String,
    pub material_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceReadPrefixesResponse {
    pub entries: Vec<ServiceBundleEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceDeleteRequest {
    pub secret_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceDeleteResponse {
    pub existed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceErrorResponse {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoresResponse {
    pub stores: Vec<String>,
}

/// Provenance of a store as visible to the admin UI. Mirrors
/// [`underwrap_protocol::config::StoreSource`] but lives here as well
/// so DTOs in this module are self-contained on the wire.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum AdminStoreSource {
    Declarative,
    Dynamic,
}

/// Storage backend kind a store uses. Kept as a flat enum so the
/// admin UI can switch on it without having to mirror the full
/// `StorageConfig` discriminant logic; the per-kind details
/// (`dir`, `key_file`) live on the detail response instead.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum AdminStoreBackendKind {
    Plain,
    Encrypted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoreListItem {
    pub name: String,
    pub source: AdminStoreSource,
    pub backend: AdminStoreBackendKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub created_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoresListResponse {
    pub items: Vec<AdminStoreListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoreDetailResponse {
    pub name: String,
    pub source: AdminStoreSource,
    pub backend: AdminStoreBackendKind,
    pub dir: String,
    /// Set when the backend is `encrypted` — the path to the master
    /// key file. Never echo the key contents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub key_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub created_at: Option<u64>,
    /// On-disk path of the YAML definition. Helpful for surfacing the
    /// `declarative_immutable` error code in the SPA.
    pub source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoreCreateRequest {
    pub name: String,
    pub backend: AdminStoreBackendKind,
    pub dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
}

/// Response from `POST /v1/admin/stores`. When the new store is
/// encrypted, `key_hex` carries the freshly generated 32-byte master
/// key (hex-encoded) and is returned **exactly once** — subsequent GETs
/// of the store never expose the key value. Plain stores have
/// `key_hex == None`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoreCreateResponse {
    pub store: AdminStoreDetailResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub key_hex: Option<String>,
}

/// Response from `POST /v1/admin/stores/{name}/rotate-key`. The new
/// key is returned exactly once. `warning` documents the v1 rotation
/// limitation: existing ciphertext encrypted with the previous key
/// is no longer readable through this backend until the operator
/// re-encrypts it offline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoreRotateKeyResponse {
    pub key_hex: String,
    pub warning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoreUpdateRequest {
    /// New description. Pass an empty string to clear it; omit to
    /// leave it unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoreDeleteResponse {
    pub ok: bool,
    /// Filesystem cleanup the operator still has to do by hand. Set
    /// when `force=true` deleted a store whose data dir is non-empty;
    /// the server intentionally never `rm -rf`s secret material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cleanup_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiWrapperCounts {
    pub locked: usize,
    pub unlocked: usize,
    pub unlocked_until: usize,
    pub per_call_approval: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiOverviewStore {
    pub store: String,
    pub resource_count: usize,
    pub wrapper_counts: AdminUiWrapperCounts,
    pub active_mutation_windows: usize,
    pub recent_audit_events: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiOverviewResponse {
    pub stores: Vec<AdminUiOverviewStore>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourcesQueryRequest {
    pub store: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub slot: Option<String>,
    #[serde(default)]
    pub mutation_state: Option<String>,
    #[serde(default = "default_page")]
    pub page: usize,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceSlots {
    pub env: bool,
    pub files: bool,
    pub libsecret: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceMutationSummary {
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceListItem {
    pub resource: String,
    pub slots: AdminUiResourceSlots,
    pub catalog_wrappers: Vec<String>,
    pub mutation: AdminUiResourceMutationSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourcesQueryResponse {
    pub items: Vec<AdminUiResourceListItem>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceGetRequest {
    pub store: String,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceEntrySummary {
    pub entry: String,
    pub secret_id: String,
    pub size_bytes: usize,
    pub is_utf8: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256_hex: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, TS)]
#[ts(export)]
pub struct AdminUiResourceSlotsDetail {
    #[serde(default)]
    pub env: Vec<AdminUiResourceEntrySummary>,
    #[serde(default)]
    pub files: Vec<AdminUiResourceEntrySummary>,
    #[serde(default)]
    pub libsecret: Vec<AdminUiResourceEntrySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceGetResponse {
    pub resource: String,
    pub slots: AdminUiResourceSlotsDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceRevealRequest {
    pub store: String,
    pub secret_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AdminUiResourceRevealResponse {
    Utf8 {
        text: String,
        size_bytes: usize,
        copy_allowed: bool,
    },
    Binary {
        base64: String,
        size_bytes: usize,
        copy_allowed: bool,
        download_recommended: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceDownloadRequest {
    pub store: String,
    pub secret_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceWriteTextRequest {
    pub store: String,
    pub wrapper: String,
    pub secret_id: String,
    pub material_utf8: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceUploadFileRequest {
    pub store: String,
    pub wrapper: String,
    pub secret_id: String,
    pub material_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceWriteResponse {
    pub entry: AdminUiResourceEntrySummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceDeleteEntryRequest {
    pub store: String,
    pub wrapper: String,
    pub secret_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiResourceDeleteEntryResponse {
    pub existed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiMutationGetRequest {
    pub store: String,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiMutationWrapperState {
    pub wrapper: String,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_epoch: Option<u64>,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiMutationGetResponse {
    pub wrappers: Vec<AdminUiMutationWrapperState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "kebab-case")]
pub enum AdminUiMutationSetMode {
    Disabled,
    Temporary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiMutationSetRequest {
    pub store: String,
    pub wrapper: String,
    pub secret_id: String,
    pub mode: AdminUiMutationSetMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_epoch: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiMutationSetResponse {
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_epoch: Option<u64>,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiWrappersQueryRequest {
    pub store: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiWrapperListItem {
    pub wrapper: String,
    pub mode: WrapperApprovalMode,
    #[serde(default)]
    pub reason: Option<String>,
    pub resource_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiWrappersQueryResponse {
    pub items: Vec<AdminUiWrapperListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiWrapperSetModeRequest {
    pub store: String,
    pub wrapper: String,
    pub mode: WrapperApprovalMode,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiWrapperSetModeResponse {
    pub wrapper: String,
    pub mode: WrapperApprovalMode,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiMatrixQueryRequest {
    pub store: String,
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub wrapper: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiMatrixRow {
    pub host: String,
    pub user: String,
    pub wrapper: String,
    pub cells: std::collections::BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiMatrixQueryResponse {
    pub operations: Vec<String>,
    pub rows: Vec<AdminUiMatrixRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiMatrixSetCellRequest {
    pub store: String,
    pub wrapper: String,
    pub resource: String,
    pub operation: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiMatrixSetCellResponse {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiAuditQueryRequest {
    pub store: String,
    #[serde(default)]
    pub operator: Option<String>,
    #[serde(default)]
    pub wrapper: Option<String>,
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub from_epoch: Option<u64>,
    #[serde(default)]
    pub to_epoch: Option<u64>,
    #[serde(default = "default_page")]
    pub page: usize,
    #[serde(default = "default_page_size")]
    pub page_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiAuditEvent {
    pub ts_epoch: u64,
    pub action: String,
    pub operator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapper: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiAuditQueryResponse {
    pub items: Vec<AdminUiAuditEvent>,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminSecretReadRequest {
    pub store: String,
    pub secret_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminSecretReadResponse {
    pub material_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminSecretWriteRequest {
    pub store: String,
    /// The wrapper context the operator is editing under. Required: the
    /// backend gates writes against `(wrapper, cred:<secret_id>, store.write)`
    /// in the policy document, so a write only succeeds when the operator
    /// has an active allow rule or grant for that tuple.
    pub wrapper: String,
    pub secret_id: String,
    pub material_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminSecretListRequest {
    pub store: String,
    pub prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminSecretListResponse {
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminSecretDeleteRequest {
    pub store: String,
    pub secret_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminSecretDeleteResponse {
    pub existed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminCatalogGetRequest {
    pub store: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminCatalogGetResponse {
    pub entries: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminPolicyGetRequest {
    pub store: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminPolicyGetResponse {
    pub document: PolicyDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminPolicyPutRequest {
    pub store: String,
    pub document: PolicyDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminWrapperModeRequest {
    pub store: String,
    pub wrapper: String,
    pub mode: WrapperApprovalMode,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminRuleUpsertRequest {
    pub store: String,
    pub rule: PolicyRule,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminRuleDeleteRequest {
    pub store: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminGrantUpsertRequest {
    pub store: String,
    pub grant: PolicyGrant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminGrantDeleteRequest {
    pub store: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminOkResponse {
    pub ok: bool,
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    50
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_structs_roundtrip_with_json() {
        let req = ServiceWriteRequest {
            secret_id: "github/token".to_string(),
            material_b64: "Z2hwX3Rlc3Q=".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ServiceWriteRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }
}
