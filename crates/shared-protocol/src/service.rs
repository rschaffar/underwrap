use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::entities::{ResourceRecord, WrapperResourcePairRecord};
use crate::policy::{PolicyDocument, WrapperPermission};
use crate::resource::ResourceSlot;

// ===== Users admin API =====
//
// User records are written and read through these DTOs. The password
// hash is never returned; the wire shape mirrors
// `crate::users::UserRecord` minus the hash field. Authorisation is
// flat: any authenticated user can perform any admin action, so the
// API does not carry a role or per-store grant.

/// One user record as visible to the admin UI. Mirrors
/// `crate::users::UserRecord` minus the password hash, which the API
/// never returns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminUserListItem {
    pub id: String,
    pub username: String,
    pub created_at: u64,
    pub password_changed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminUsersListResponse {
    pub users: Vec<AdminUserListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminUserDetailResponse {
    pub id: String,
    pub username: String,
    pub created_at: u64,
    pub password_changed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminUserCreateRequest {
    pub username: String,
    /// Plaintext on the wire (over TLS). Hashed server-side with
    /// Argon2id before any storage step. Never echoed back.
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminUserUpdateRequest {
    /// New username. Omit to keep the existing one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminUserResetPasswordRequest {
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminOkUserResponse {
    pub ok: bool,
}

/// Self-service password change for the currently authenticated user.
/// Requires the current password as proof-of-possession of the
/// session. Distinct from `POST /v1/admin/users/{id}/password`, which
/// is the admin reset path and doesn't require the target's old
/// password.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminPasswordChangeRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct ServiceErrorResponse {
    pub code: String,
    pub message: String,
}

/// Storage backend kind a store uses. Kept as a flat enum so the
/// admin UI can switch on it without having to mirror the full
/// `StorageConfig` discriminant logic; per-kind details (currently
/// only `dir`) live on the detail response.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum AdminStoreBackendKind {
    Plain,
    Encrypted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminStoreUsageSummary {
    /// How many runtime client instances currently map to this store.
    pub mapped_client_instance_count: u32,
    /// Whether the store currently contains at least one secret.
    pub has_secrets: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminStoreActionState {
    /// Whether this principal may edit the store metadata.
    pub can_edit_metadata: bool,
    /// Whether this principal may rotate the store's master key.
    /// Rotation requires the store to be encrypted **and** currently
    /// unlocked.
    pub can_rotate_key: bool,
    /// Whether this principal may unlock the store. True only for
    /// encrypted stores that are currently locked.
    pub can_unlock: bool,
    /// Whether this principal may lock the store. True only for
    /// encrypted stores that are currently unlocked.
    pub can_lock: bool,
    /// Whether a plain DELETE is currently allowed.
    pub can_delete_now: bool,
    /// Whether a force delete is currently allowed.
    pub can_force_delete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
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
    /// `true` if this is an encrypted store with no master key loaded.
    /// Plain stores are never locked.
    pub locked: bool,
    pub usage: AdminStoreUsageSummary,
    pub actions: AdminStoreActionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminStoresListResponse {
    pub items: Vec<AdminStoreListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminStoreDetailResponse {
    /// Stable internal identifier used for routing and runtime mapping.
    pub id: String,
    /// Editable operator-facing display name.
    pub name: String,
    pub backend: AdminStoreBackendKind,
    pub dir: String,
    /// `true` if this is an encrypted store with no master key loaded.
    /// Plain stores are never locked.
    pub locked: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminStoreCreateResponse {
    pub store: AdminStoreDetailResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub key_hex: Option<String>,
}

/// Response from `POST /v1/admin/stores/{name}/rotate-key`. The new
/// key is returned exactly once. Every resource in the store has
/// been transparently re-encrypted under the new key before the
/// response is sent; `warning` carries a one-line reminder that the
/// operator is solely responsible for saving the new value (the
/// backend does not store it on disk).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminStoreRotateKeyResponse {
    pub key_hex: String,
    pub warning: String,
}

/// Body of `POST /v1/admin/stores/{id}/unlock`. The operator pastes
/// in the 64-character hex master key that was issued at store
/// creation (or rotation). Whitespace around the value is tolerated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminStoreUnlockRequest {
    pub key_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
#[serde(rename_all = "kebab-case")]
pub enum AdminClientInstanceStatus {
    Active,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminClientInstancesListResponse {
    pub items: Vec<AdminClientInstanceListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminUiWrapperCounts {
    pub locked: usize,
    pub unlocked: usize,
    pub unlocked_until: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminUiOverviewStore {
    pub store: String,
    pub resource_count: usize,
    pub wrapper_counts: AdminUiWrapperCounts,
    /// Number of wrapper permissions with `writable = true`. Surfaced
    /// on the dashboard overview as a quick "how many wrapper writes
    /// are open" signal.
    pub writable_permission_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminUiOverviewResponse {
    pub stores: Vec<AdminUiOverviewStore>,
}

// ===== Secret CRUD =====
//
// Per-slot entries inside a resource blob are addressed by the
// structured tuple `(store, resource, slot, key)`. The on-disk format
// is unchanged — handlers compose the legacy in-blob entry key via
// `resource::compose_secret_id` internally. The wire form no longer
// carries a flattened `secret_id` string, which removes the
// "first-match `/<slot>/` wins" parsing ambiguity from the admin
// surface and lets us validate keys per slot.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminSecretReadRequest {
    pub store: String,
    pub resource_id: String,
    pub slot: ResourceSlot,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminSecretReadResponse {
    pub material_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminSecretWriteRequest {
    pub store: String,
    pub resource_id: String,
    pub slot: ResourceSlot,
    pub key: String,
    pub material_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminSecretListRequest {
    pub store: String,
    /// Restrict listing to a single resource. `None` returns entries
    /// for every resource in the store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resource_id: Option<String>,
}

/// One per-slot entry as listed by the admin surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminSecretEntry {
    pub resource_id: String,
    pub slot: ResourceSlot,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminSecretListResponse {
    pub entries: Vec<AdminSecretEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminSecretDeleteRequest {
    pub store: String,
    pub resource_id: String,
    pub slot: ResourceSlot,
    pub key: String,
}

/// Move an entry within a resource blob from `from_key` to `to_key`,
/// optionally overwriting its material. The whole operation is one
/// atomic load-modify-save of the resource blob — there is no
/// intermediate "both entries exist" window.
///
/// `material_b64 = None` means "keep the existing material under
/// `from_key`"; `Some(_)` overwrites in the same step. Rename refuses
/// when `to_key` already exists (callers must delete it explicitly).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminSecretRenameRequest {
    pub store: String,
    pub resource_id: String,
    pub slot: ResourceSlot,
    pub from_key: String,
    pub to_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub material_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminSecretDeleteResponse {
    pub existed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminPolicyGetRequest {
    pub store: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminPolicyGetResponse {
    pub document: PolicyDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminPolicyPutRequest {
    pub store: String,
    pub document: PolicyDocument,
}

// ===== Wrapper permission CRUD =====
//
// One `WrapperPermission` per `(client_instance, wrapper, resource)`
// tuple. The mode controls *if* the permission is usable; the
// `writable` flag on the permission decides wrapper-side writes for
// the `files`/`libsecret` slots.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminWrapperPermissionsListRequest {
    pub store: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminWrapperPermissionsListResponse {
    pub permissions: Vec<WrapperPermission>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminWrapperPermissionUpsertRequest {
    pub store: String,
    pub permission: WrapperPermission,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminWrapperPermissionDeleteRequest {
    pub store: String,
    pub client_instance: String,
    pub wrapper: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminOkResponse {
    pub ok: bool,
}

// ===== Admin resource and wrapper/resource-pair CRUD =====
//
// One Resource and one Pair collection per store. Delete refuses when
// any dependent exists; the refusal response carries enough structured
// detail for the SPA to point the operator at the exact blocker.

/// The composite key for a wrapper permission. Used in delete-refusal
/// responses so the SPA can render a "you must clear these wrapper
/// permissions first" list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminWrapperPermissionKey {
    pub client_instance: String,
    pub wrapper: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourcesListRequest {
    pub store: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourcesListResponse {
    pub resources: Vec<ResourceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourceCreateRequest {
    pub store: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourceCreateResponse {
    pub resource: ResourceRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourceDeleteRequest {
    pub store: String,
    pub id: String,
}

/// Rename an existing resource by id. The id stays stable; the name
/// changes. Returns the updated record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourceRenameRequest {
    pub store: String,
    pub id: String,
    pub new_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourceRenameResponse {
    pub resource: ResourceRecord,
}

/// Request whole-blob get/put for a resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourceBlobGetRequest {
    pub store: String,
    pub id: String,
}

/// JCS-canonical JSON bytes of the resource blob, base64-encoded for
/// HTTP transport.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourceBlobGetResponse {
    pub blob_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourceBlobPutRequest {
    pub store: String,
    pub id: String,
    pub blob_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourceBlobPutResponse {
    pub ok: bool,
}

/// Counts plus a short sample of names per dependent kind. The sample
/// is capped server-side so a heavily-used resource doesn't return
/// thousands of strings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema, Default)]
#[ts(export)]
pub struct AdminResourceDeleteBlockers {
    pub secret_count: u32,
    #[serde(default)]
    pub sample_secret_ids: Vec<String>,
    pub pair_count: u32,
    #[serde(default)]
    pub sample_pairs: Vec<ServiceWrapperResourcePair>,
    pub permission_count: u32,
    #[serde(default)]
    pub sample_permissions: Vec<AdminWrapperPermissionKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminResourceDeleteResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub blocked_by: Option<AdminResourceDeleteBlockers>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminPairsListRequest {
    pub store: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminPairsListResponse {
    pub pairs: Vec<WrapperResourcePairRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminPairCreateRequest {
    pub store: String,
    pub wrapper: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminPairCreateResponse {
    pub pair: WrapperResourcePairRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminPairDeleteRequest {
    pub store: String,
    pub wrapper: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema, Default)]
#[ts(export)]
pub struct AdminPairDeleteBlockers {
    pub permission_count: u32,
    #[serde(default)]
    pub sample_permissions: Vec<AdminWrapperPermissionKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct AdminPairDeleteResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub blocked_by: Option<AdminPairDeleteBlockers>,
}

/// One `(wrapper, resource)` pair on the admin wire. Used in
/// resource-delete blocker payloads to name the dependent pairs that
/// would have to be cleaned up before the delete can proceed. The
/// persisted record (`WrapperResourcePairRecord`) adds provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, utoipa::ToSchema)]
#[ts(export)]
pub struct ServiceWrapperResourcePair {
    pub wrapper: String,
    pub resource: String,
}

// ===== Runtime API (resource-vocabulary; replaces the legacy
// per-key /v1/read /v1/write /v1/list /v1/delete /v1/read-prefixes
// endpoints and /v1/runtime/wrappers/register). =====
//
// The runtime client addresses wrappers and resources by **name** only.
// There is no `secret_id` field anywhere on this surface and no way to
// address `__underwrap/*` or `<resource>/<slot>/<key>` storage keys.
// The backend re-derives the principal from the mTLS cert and
// re-evaluates the policy document on every call; there are no
// sessions or open-call state between open and finalize.

/// `POST /v1/wrappers` — one `(wrapper, resource)` pair on the wire.
/// Mount paths are intentionally absent: they are a daemon-local fact
/// used to wire bind mounts and never need to reach the backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeWrapperRegisterPair {
    pub wrapper_name: String,
    pub resource_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeWrapperRegisterRequest {
    pub wrappers: Vec<RuntimeWrapperRegisterPair>,
}

/// Response to `POST /v1/wrappers`.
///
/// The `outcome` tag makes the JSON unambiguous on the wire. `Ok`
/// reports anything the call newly created; `AutoCreateDisabled`
/// reports the would-be creations so an operator can decide whether
/// to flip the auto-create flag on the calling client instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum RuntimeWrapperRegisterResponse {
    Ok {
        /// Resource names that were newly created on this call. Empty
        /// when every named resource already existed.
        #[serde(default)]
        auto_created_resources: Vec<String>,
        /// Pairs that were newly created on this call.
        #[serde(default)]
        auto_created_pairs: Vec<RuntimeWrapperRegisterPair>,
    },
    AutoCreateDisabled {
        missing_resources: Vec<String>,
        missing_pairs: Vec<RuntimeWrapperRegisterPair>,
    },
}

/// `POST /v1/wrapper-invocations` — open an invocation. Stateless:
/// the backend doesn't allocate a session, it just re-evaluates the
/// policy and returns the current blob.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeWrapperInvocationOpenRequest {
    pub wrapper_name: String,
    pub resource_name: String,
}

/// Response to `POST /v1/wrapper-invocations`.
///
/// `Deny.reason` is a stable kebab-case discriminator (`no-permission`,
/// `locked`, `expired`, `unregistered-pair`) the daemon can switch on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum RuntimeWrapperInvocationOpenResponse {
    Allow {
        /// Stable backend resource id (e.g. `res_<hex>`). The daemon
        /// relays this to the wrapper inside `WrapperResponse::ReadOk`
        /// as an opaque identifier; the daemon itself never has to
        /// look it up.
        resource_id: String,
        /// Canonical resource blob, base64-encoded.
        blob_b64: String,
        /// Whether the matched permission allows wrapper-side writes.
        /// The daemon uses this only as a hint to fail closed early;
        /// the backend re-checks on finalize regardless.
        writable: bool,
    },
    Deny {
        reason: String,
    },
}

/// `POST /v1/wrapper-invocations/finalize` — close an invocation.
/// Optional writeback blob, plus an unconditional `last_used_at` bump
/// on the matched pair.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeWrapperInvocationFinalizeRequest {
    pub wrapper_name: String,
    pub resource_name: String,
    /// `Some` = writeback. The wrapper sends the entire updated blob,
    /// not a diff. `None` = usage signal only (still bumps last_used_at).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_b64: Option<String>,
}

/// Response to `POST /v1/wrapper-invocations/finalize`.
///
/// `Deny` covers policy denials. `InvalidBlob` covers writeback
/// validation failures (`env-section-immutable`, `noncanonical-blob`,
/// `schema-unknown`, `decode-failed`). They are split apart because a
/// `Deny` may flip to `Allow` after an operator edits the policy,
/// whereas `InvalidBlob` is a client bug that won't change on retry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum RuntimeWrapperInvocationFinalizeResponse {
    Ok,
    Deny { reason: String },
    InvalidBlob { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_register_request_roundtrips() {
        let req = RuntimeWrapperRegisterRequest {
            wrappers: vec![RuntimeWrapperRegisterPair {
                wrapper_name: "gh".into(),
                resource_name: "github/token".into(),
            }],
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: RuntimeWrapperRegisterRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, req);
    }
}
