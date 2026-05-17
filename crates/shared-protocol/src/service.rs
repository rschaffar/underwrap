use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::policy::{AgentGrant, PolicyDocument};

// ===== Users admin API (ticket 240) =====
//
// Operator records are written and read through these DTOs. The
// password hash is never returned; the wire shape mirrors
// `crate::operators::OperatorRecord` minus the hash field. The
// `AdminUserRole` enum is duplicated here (rather than re-exported
// from the backendd crate) so the shared-protocol crate stays free of
// the backend dependency direction.

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum AdminUserRole {
    Admin,
    Operator,
}

/// One operator record as visible to the admin UI. Mirrors
/// `crate::operators::OperatorRecord` minus the password hash, which
/// the API never returns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUserListItem {
    pub id: String,
    pub username: String,
    pub role: AdminUserRole,
    #[serde(default)]
    pub granted_stores: Vec<String>,
    pub created_at: u64,
    pub password_changed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUsersListResponse {
    pub users: Vec<AdminUserListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUserDetailResponse {
    pub id: String,
    pub username: String,
    pub role: AdminUserRole,
    #[serde(default)]
    pub granted_stores: Vec<String>,
    pub created_at: u64,
    pub password_changed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUserCreateRequest {
    pub username: String,
    /// Plaintext on the wire (over TLS). Hashed server-side with
    /// Argon2id before any storage step. Never echoed back.
    pub password: String,
    pub role: AdminUserRole,
    #[serde(default)]
    pub granted_stores: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUserUpdateRequest {
    /// New username. Omit to keep the existing one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
    /// New role. Omit to keep the existing one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub role: Option<AdminUserRole>,
    /// Replacement grant list. Omit to leave existing grants
    /// unchanged. The empty list is a meaningful value (revoke all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub granted_stores: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUserResetPasswordRequest {
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminOkUserResponse {
    pub ok: bool,
}

/// Self-service password change for the currently authenticated
/// operator. Requires the current password as proof-of-possession of
/// the session. Distinct from `POST /v1/admin/users/{id}/password`,
/// which is admin-only and doesn't require the target's old password.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminPasswordChangeRequest {
    pub current_password: String,
    pub new_password: String,
}

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
    /// Stable internal identifier used for routing and runtime mapping.
    pub id: String,
    /// Editable operator-facing display name.
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
    /// Stable internal identifier used for routing and runtime mapping.
    pub id: String,
    /// Editable operator-facing display name.
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
    /// New display name. Pass an empty string to clear it (invalid);
    /// omit to leave it unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "kebab-case")]
pub enum AdminClientInstanceStatus {
    Active,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminClientInstanceListItem {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hostname_hint: Option<String>,
    pub instance_name: String,
    pub current_store_id: String,
    pub status: AdminClientInstanceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_seen_at: Option<u64>,
    pub current_credential_fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminClientInstancesListResponse {
    pub items: Vec<AdminClientInstanceListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminClientInstanceDetailResponse {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hostname_hint: Option<String>,
    pub instance_name: String,
    pub current_store_id: String,
    pub status: AdminClientInstanceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_seen_at: Option<u64>,
    pub current_credential_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub current_credential_issued_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminClientInstanceCreateRequest {
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hostname_hint: Option<String>,
    pub instance_name: String,
    pub current_store_id: String,
    pub status: AdminClientInstanceStatus,
    pub cert_pem: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub issued_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminClientInstanceUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hostname_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub current_store_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<AdminClientInstanceStatus>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "kebab-case")]
pub enum AdminEnrollmentInviteStatus {
    Pending,
    Consumed,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminEnrollmentInviteListItem {
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hostname_hint: Option<String>,
    pub instance_name: String,
    pub current_store_id: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub status: AdminEnrollmentInviteStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub consumed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub consumed_client_instance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminEnrollmentInvitesListResponse {
    pub items: Vec<AdminEnrollmentInviteListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminEnrollmentInviteCreateRequest {
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hostname_hint: Option<String>,
    pub instance_name: String,
    pub current_store_id: String,
    /// Optional invite lifetime override in seconds. The backend may cap this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expires_in_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminEnrollmentInviteCreateResponse {
    pub invite: AdminEnrollmentInviteListItem,
    /// Returned exactly once. The CLI prompts for this value interactively;
    /// do not place it in argv or shell history.
    pub enrollment_secret: String,
    pub enrollment_url: String,
    pub enrollment_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnrollmentSubmitRequest {
    pub invite_id: String,
    pub enrollment_secret: String,
    pub csr_pem: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnrollmentSubmitResponse {
    pub client_instance_id: String,
    pub cert_pem: String,
    pub ca_cert_pem: String,
    pub cert_fingerprint: String,
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
    /// Number of resources with the wrapper-side `files` slot
    /// writable. Surfaced on the dashboard overview as a quick
    /// "how many resources have writes opened up" signal.
    pub writable_files_resources: usize,
    /// Same for the `libsecret` slot.
    pub writable_libsecret_resources: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminUiOverviewResponse {
    pub stores: Vec<AdminUiOverviewStore>,
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

// ===== Agent grant CRUD (ticket 246) =====
//
// Replaces the pre-245 per-secret `(wrapper, resource, operation)` rule
// and grant model. One `AgentGrant` per `(wrapper, host, user)` tuple,
// keyed on that triple; mode controls *if* the grant is usable.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminAgentGrantsListRequest {
    pub store: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminAgentGrantsListResponse {
    pub grants: Vec<AgentGrant>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminAgentGrantUpsertRequest {
    pub store: String,
    pub grant: AgentGrant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminAgentGrantDeleteRequest {
    pub store: String,
    pub wrapper: String,
    pub host: String,
    pub user: String,
}

// ===== Per-resource wrapper-write permissions =====

/// Get the current `(files_writable, libsecret_writable)` maps for a store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminWritePermissionsGetRequest {
    pub store: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminWritePermissionsResponse {
    pub store: String,
    /// Resource → allowed flag. Resources missing from the map are
    /// implicitly denied — same semantics as `PolicyDocument`.
    pub files_writable: std::collections::HashMap<String, bool>,
    pub libsecret_writable: std::collections::HashMap<String, bool>,
}

/// Set a single `(resource, slot) → enabled` permission. The slot is
/// constrained to `files` or `libsecret` — `env` has no wrapper-side
/// write path so it isn't valid here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminWritePermissionsSetRequest {
    pub store: String,
    pub resource: String,
    pub slot: AdminWritableSlot,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(rename_all = "kebab-case")]
pub enum AdminWritableSlot {
    Files,
    Libsecret,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminOkResponse {
    pub ok: bool,
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
