use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::entities::{ResourceRecord, WrapperResourcePairRecord};
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

// ===== Runtime wrapper register =====
//
// `POST /v1/runtime/wrappers/register` is the auto-create endpoint
// wrappers hit at daemon startup. The backend reads the calling
// client-instance's `auto_create_wrapper_assets` flag to decide whether
// missing resources and pairs may be inserted on this principal's
// behalf. The endpoint is upsert-only — existing entities are never
// modified, removals never happen, and provenance is set once on
// first sight.

/// One `(wrapper, resource)` pair on the wire. The minimal payload
/// needed to drive auto-create; the persisted shape adds provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct ServiceWrapperResourcePair {
    pub wrapper: String,
    pub resource: String,
    /// Target-home-relative mount paths the wrapper bind-mounts this
    /// resource's `files` slot at. Empty when the wrapper does not
    /// project `files` for this resource. Mount paths are runtime
    /// metadata: the backend persists them on the pair record so the
    /// admin SPA can render the resolved location of a stored file.
    #[serde(default)]
    pub files_mount_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceWrapperRegisterRequest {
    pub pairs: Vec<ServiceWrapperResourcePair>,
}

/// Reason a register call was rejected at the policy gate. Today the
/// only reason is "this client instance is not opted-in"; the variant
/// is structured so we can add more reasons without breaking clients.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ServiceWrapperRegisterReject {
    /// The calling client instance does not have
    /// `auto_create_wrapper_assets` enabled. Lists the entities that
    /// would have been created so the operator can see what's missing
    /// without flipping the flag blindly.
    AutoCreateDisabled {
        missing_resources: Vec<String>,
        missing_pairs: Vec<ServiceWrapperResourcePair>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ServiceWrapperRegisterResponse {
    /// Resources that were newly created on this call. Empty when
    /// every named resource already existed.
    #[serde(default)]
    pub created_resources: Vec<String>,
    /// Pairs that were newly created on this call.
    #[serde(default)]
    pub created_pairs: Vec<ServiceWrapperResourcePair>,
    /// Populated when the call was refused; in that case no
    /// `created_*` entries are produced and the backend writes
    /// nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected: Option<ServiceWrapperRegisterReject>,
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
pub struct AdminStoreUsageSummary {
    /// How many runtime client instances currently map to this store.
    pub mapped_client_instance_count: u32,
    /// Whether the store currently contains at least one secret.
    pub has_secrets: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoreActionState {
    /// Whether this principal may edit the store metadata.
    pub can_edit_metadata: bool,
    /// Whether this principal may rotate the store's master key.
    pub can_rotate_key: bool,
    /// Whether a plain DELETE is currently allowed.
    pub can_delete_now: bool,
    /// Whether a force delete is currently allowed.
    pub can_force_delete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoreListItem {
    /// Stable internal identifier used for routing and runtime mapping.
    pub id: String,
    /// Editable operator-facing display name.
    pub name: String,
    pub backend: AdminStoreBackendKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub created_at: Option<u64>,
    pub usage: AdminStoreUsageSummary,
    pub actions: AdminStoreActionState,
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
    /// On-disk path of the backend-managed store registry file that
    /// owns this record.
    pub source_path: String,
    pub usage: AdminStoreUsageSummary,
    pub actions: AdminStoreActionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminStoreCreateRequest {
    pub name: String,
    pub backend: AdminStoreBackendKind,
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
pub struct AdminClientInstanceActionState {
    /// Whether this principal may edit the display-name metadata.
    pub can_edit_display_name: bool,
    /// Whether this principal may remap the instance to another store.
    pub can_remap_store: bool,
    /// Whether this principal may transition the instance into the
    /// disabled state.
    pub can_disable: bool,
    /// Whether this principal may transition the instance back to the
    /// active state.
    pub can_enable: bool,
    /// Whether this principal may delete the instance record.
    pub can_delete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminClientInstanceListItem {
    pub id: String,
    pub display_name: String,
    pub current_store_id: String,
    pub current_store_name: String,
    pub status: AdminClientInstanceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_seen_at: Option<u64>,
    pub current_credential_fingerprint: String,
    /// Whether this instance is allowed to auto-create resource and
    /// wrapper/resource-pair records when its wrappers register on
    /// startup. Defaults to `false` for newly-created instances.
    #[serde(default)]
    pub auto_create_wrapper_assets: bool,
    pub actions: AdminClientInstanceActionState,
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
    pub current_store_id: String,
    pub current_store_name: String,
    pub status: AdminClientInstanceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_seen_at: Option<u64>,
    pub current_credential_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub current_credential_issued_at: Option<u64>,
    /// Whether this instance is allowed to auto-create resource and
    /// wrapper/resource-pair records on the runtime register endpoint.
    #[serde(default)]
    pub auto_create_wrapper_assets: bool,
    pub actions: AdminClientInstanceActionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminClientInstanceCreateRequest {
    pub display_name: String,
    pub current_store_id: String,
    pub status: AdminClientInstanceStatus,
    pub cert_pem: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub issued_at: Option<u64>,
    /// Initial value for `auto_create_wrapper_assets`. Defaults to
    /// `false` when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub auto_create_wrapper_assets: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminClientInstanceUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub current_store_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<AdminClientInstanceStatus>,
    /// Optional replacement client cert PEM. When set, the backend
    /// parses it, validates it as a fresh client cert, replaces
    /// `current_credential`, and updates the stored fingerprint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cert_pem: Option<String>,
    /// New value for `auto_create_wrapper_assets`. Omit to leave
    /// unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub auto_create_wrapper_assets: Option<bool>,
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
    /// Number of grants with `writable = true`. Surfaced on the
    /// dashboard overview as a quick "how many wrapper writes are
    /// open" signal.
    pub writable_grant_count: usize,
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

// ===== Agent grant CRUD =====
//
// One `AgentGrant` per `(client_instance, wrapper, resource)` tuple.
// The mode controls *if* the grant is usable; the `writable` flag on
// the grant decides wrapper-side writes for the `files`/`libsecret`
// slots.

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
    pub client_instance: String,
    pub wrapper: String,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminOkResponse {
    pub ok: bool,
}

// ===== Admin resource and wrapper/resource-pair CRUD =====
//
// One Resource and one Pair collection per store. Delete refuses when
// any dependent exists; the refusal response carries enough structured
// detail for the SPA to point the operator at the exact blocker.

/// The composite key for a grant. Used in delete-refusal responses so
/// the SPA can render a "you must clear these grants first" list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminGrantKey {
    pub client_instance: String,
    pub wrapper: String,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminResourcesListRequest {
    pub store: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminResourcesListResponse {
    pub resources: Vec<ResourceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminResourceCreateRequest {
    pub store: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminResourceCreateResponse {
    pub resource: ResourceRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminResourceDeleteRequest {
    pub store: String,
    pub name: String,
}

/// Request whole-blob get/put for a resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminResourceBlobGetRequest {
    pub store: String,
    pub name: String,
}

/// JCS-canonical JSON bytes of the resource blob, base64-encoded for
/// HTTP transport.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminResourceBlobGetResponse {
    pub blob_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminResourceBlobPutRequest {
    pub store: String,
    pub name: String,
    pub blob_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminResourceBlobPutResponse {
    pub ok: bool,
}

/// Counts plus a short sample of names per dependent kind. The sample
/// is capped server-side so a heavily-used resource doesn't return
/// thousands of strings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, Default)]
#[ts(export)]
pub struct AdminResourceDeleteBlockers {
    pub secret_count: u32,
    #[serde(default)]
    pub sample_secret_ids: Vec<String>,
    pub pair_count: u32,
    #[serde(default)]
    pub sample_pairs: Vec<ServiceWrapperResourcePair>,
    pub grant_count: u32,
    #[serde(default)]
    pub sample_grants: Vec<AdminGrantKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminResourceDeleteResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub blocked_by: Option<AdminResourceDeleteBlockers>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminPairsListRequest {
    pub store: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminPairsListResponse {
    pub pairs: Vec<WrapperResourcePairRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminPairCreateRequest {
    pub store: String,
    pub wrapper: String,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminPairCreateResponse {
    pub pair: WrapperResourcePairRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminPairDeleteRequest {
    pub store: String,
    pub wrapper: String,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, Default)]
#[ts(export)]
pub struct AdminPairDeleteBlockers {
    pub grant_count: u32,
    #[serde(default)]
    pub sample_grants: Vec<AdminGrantKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct AdminPairDeleteResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub blocked_by: Option<AdminPairDeleteBlockers>,
}

// `ServiceWrapperResourcePair` is shared between the runtime register
// endpoint and the admin delete-blocker shape; the persisted record
// adds provenance fields on top.

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
