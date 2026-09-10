//! Admin API DTOs — the admin SPA ↔ backend wire, authenticated by
//! session cookie + CSRF. Every type here derives `utoipa::ToSchema` and
//! feeds the OpenAPI export the SPA codegen consumes. Authorisation is
//! flat: any authenticated user can perform any admin action, so the API
//! carries no role or per-store grant.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::Zeroizing;

use crate::entities::{ResourceRecord, WrapperResourcePairRecord};
use crate::policy::WrapperPermission;
use crate::resource::KvSecretSlot;
use crate::resource_blob::ResourceContentReference;

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminUserListItem {
    pub id: String,
    pub username: String,
    pub created_at: u64,
    pub password_changed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminUsersListResponse {
    pub users: Vec<AdminUserListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminUserCreateRequest {
    pub username: String,
    /// Plaintext on the wire (over TLS). Hashed server-side with
    /// Argon2id before any storage step. Never echoed back.
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminUserUpdateRequest {
    /// New username. Omit to keep the existing one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminUserResetPasswordRequest {
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminOkUserResponse {
    pub ok: bool,
}

/// Self-service password change for the currently authenticated user.
/// Requires the current password as proof-of-possession of the
/// session. Distinct from `POST /v1/admin/users/{id}/password`, which
/// is the admin reset path and doesn't require the target's old
/// password.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminPasswordChangeRequest {
    pub current_password: String,
    pub new_password: String,
}

/// Storage backend kind, independent of lifecycle state and action availability.
/// The admin API does not expose a separate store-detail endpoint or the
/// configured data directory in store list items.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AdminStoreBackendKind {
    Plain,
    Encrypted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreUsageSummary {
    /// How many runtime client instances currently map to this store.
    pub mapped_client_instance_count: u32,
    /// Whether inspection found a non-control storage entry.
    /// False also represents unavailable inspection for a locked, busy, or
    /// recovery-required store; use lifecycle state and server-derived actions
    /// rather than interpreting that value as proof of an empty store.
    pub has_secrets: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AdminStoreLifecycleState {
    Ready,
    Unlocked,
    Locked,
    Busy,
    RecoveryRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreActionState {
    pub can_update: bool,
    pub can_migrate: bool,
    pub can_rotate_key: bool,
    pub can_unlock: bool,
    pub can_lock: bool,
    pub can_delete_now: bool,
    pub can_force_delete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreListItem {
    /// Stable internal identifier used for routing and runtime mapping.
    pub id: String,
    /// Editable operator-facing display name.
    pub name: String,
    pub backend: AdminStoreBackendKind,
    pub lifecycle_state: AdminStoreLifecycleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,
    pub usage: AdminStoreUsageSummary,
    pub actions: AdminStoreActionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoresListResponse {
    pub items: Vec<AdminStoreListItem>,
}

/// Owned admin key material.
///
/// Its wire and OpenAPI projections are ordinary strings, while formatting is
/// redacted and the owned allocation is zeroized when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct AdminKeyHex(Zeroizing<String>);

impl AdminKeyHex {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for AdminKeyHex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AdminKeyHex(<redacted>)")
    }
}

impl Serialize for AdminKeyHex {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.expose())
    }
}

impl<'de> Deserialize<'de> for AdminKeyHex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::new)
    }
}

#[cfg(feature = "full")]
impl utoipa::PartialSchema for AdminKeyHex {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        utoipa::openapi::schema::Object::with_type(utoipa::openapi::schema::Type::String).into()
    }
}

#[cfg(feature = "full")]
impl utoipa::ToSchema for AdminKeyHex {}

fn deserialize_present_admin_key_hex<'de, D>(
    deserializer: D,
) -> Result<Option<AdminKeyHex>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<AdminKeyHex>::deserialize(deserializer)?
        .map(Some)
        .ok_or_else(|| {
            <D::Error as serde::de::Error>::custom("key_hex must be omitted instead of set to null")
        })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreKeyResponse {
    pub key_hex: AdminKeyHex,
}

/// Store creation request.
///
/// Encrypted creation requires `key_hex` to be exactly 64 lowercase
/// hexadecimal characters without trimming or normalization.
/// Plain creation requires `key_hex` to be omitted; explicit JSON `null` is
/// invalid for every backend kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreCreateRequest {
    pub name: String,
    pub backend: AdminStoreBackendKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Required exactly when `backend` is `encrypted`; otherwise omitted.
    /// The accepted value is exactly 64 lowercase hexadecimal characters.
    #[serde(
        default,
        deserialize_with = "deserialize_present_admin_key_hex",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(value_type = String, nullable = false)]
    pub key_hex: Option<AdminKeyHex>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreCreateResponse {
    pub store: AdminStoreListItem,
}

/// Store backend migration request.
///
/// Migration to encrypted requires `key_hex` to be exactly 64 lowercase
/// hexadecimal characters without trimming or normalization.
/// Migration to plain requires `key_hex` to be omitted; explicit JSON `null`
/// is invalid for every migration direction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreMigrateRequest {
    pub backend: AdminStoreBackendKind,
    /// Required exactly when `backend` is `encrypted`; otherwise omitted.
    /// The accepted value is exactly 64 lowercase hexadecimal characters.
    #[serde(
        default,
        deserialize_with = "deserialize_present_admin_key_hex",
        skip_serializing_if = "Option::is_none"
    )]
    #[schema(value_type = String, nullable = false)]
    pub key_hex: Option<AdminKeyHex>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreMigrateResponse {
    pub store: AdminStoreListItem,
}

/// Response from `POST /v1/admin/stores/{name}/rotate-key`. The new
/// key is returned exactly once. Every resource is re-encrypted before
/// the response is sent, while the displaced ciphertext generation remains
/// in a uniquely named backend-host recovery workspace until trusted manual
/// cleanup. `warning` explains that response delivery and local browser
/// acknowledgement do not prove external key custody. The backend never
/// persists the replacement key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreRotateKeyResponse {
    pub key_hex: String,
    pub warning: String,
}

/// Body of `POST /v1/admin/stores/{id}/unlock`. The operator pastes
/// in the 64-character hex master key that was issued at store
/// creation (or rotation). Whitespace around the value is tolerated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreUnlockRequest {
    pub key_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreUpdateRequest {
    /// New display name. Pass an empty string to clear it (invalid);
    /// omit to leave it unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// New description. Pass an empty string to clear it; omit to
    /// leave it unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Response of `POST /v1/admin/stores/{id}/lock`. Carries the
/// idempotence flag so the SPA can distinguish "we just locked the
/// store" from "store was already locked when we got here".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreLockResponse {
    pub ok: bool,
    pub was_locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreDeleteResponse {
    pub ok: bool,
    /// Filesystem cleanup the operator still has to do by hand. Set
    /// when `force=true` deleted a store whose data dir is non-empty;
    /// the server intentionally never `rm -rf`s secret material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_hint: Option<AdminStoreDeleteCleanupHint>,
}

/// Structured hint for the manual cleanup the operator still has to
/// perform after a force-delete: the data dir was left in place so
/// the operator can decide whether to back it up, and the command
/// they would run to remove it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminStoreDeleteCleanupHint {
    pub data_dir: String,
    pub command: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AdminClientInstanceStatus {
    Active,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminClientInstanceActionState {
    /// Whether this principal may transition the instance into the
    /// disabled state.
    pub can_disable: bool,
    /// Whether this principal may transition the instance back to the
    /// active state.
    pub can_enable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminClientInstanceListItem {
    pub id: String,
    pub display_name: String,
    pub current_store_id: String,
    pub current_store_name: String,
    pub status: AdminClientInstanceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<u64>,
    pub current_credential_fingerprint: String,
    /// Whether this instance is allowed to auto-create resource and
    /// wrapper/resource-pair records when its wrappers register on
    /// startup. Defaults to `false` for newly-created instances.
    #[serde(default)]
    pub auto_create_wrapper_assets: bool,
    pub actions: AdminClientInstanceActionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminClientInstancesListResponse {
    pub items: Vec<AdminClientInstanceListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminClientInstanceCreateRequest {
    pub display_name: String,
    pub current_store_id: String,
    pub status: AdminClientInstanceStatus,
    pub cert_pem: String,
    /// Initial value for `auto_create_wrapper_assets`. Defaults to
    /// `false` when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_create_wrapper_assets: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminClientInstanceUpdateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_store_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<AdminClientInstanceStatus>,
    /// Optional replacement client cert PEM. When set, the backend
    /// parses it, validates it as a fresh client cert, replaces
    /// `current_credential`, and updates the stored fingerprint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert_pem: Option<String>,
    /// New value for `auto_create_wrapper_assets`. Omit to leave
    /// unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_create_wrapper_assets: Option<bool>,
}

// ===== Secret CRUD =====
//
// Per-slot entries inside a resource blob are addressed by the
// structured tuple `(store, resource, slot, key)`. Store id, resource
// id, slot, and key all travel in the URL path; bodies carry only the
// value-bearing fields (material, rename target). The on-disk format
// is unchanged — handlers compose the in-blob entry key via
// `resource::compose_secret_id` internally.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminSecretReadResponse {
    pub material_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminSecretWriteRequest {
    pub material_b64: String,
}

/// One per-slot entry as listed by the admin surface. The flat-KV
/// admin endpoints (`env`, `files`) emit these; the `libsecret` slot
/// is structurally a tree and is addressed by its own endpoints, so
/// it never appears here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminSecretEntry {
    pub resource_id: String,
    pub slot: KvSecretSlot,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminSecretListResponse {
    pub entries: Vec<AdminSecretEntry>,
}

/// Move an entry within a resource blob from the URL-addressed
/// `(resource_id, slot, from_key)` to `to_key` under the same slot,
/// optionally overwriting its material. The modified blob is persisted with
/// atomic replacement visibility, without an intermediate two-key state.
/// Loading, modifying, and saving are not a transaction: concurrent whole-blob
/// writers remain last-writer-wins.
///
/// `material_b64 = None` means "keep the existing material"; `Some(_)`
/// overwrites in the same replacement. A different existing destination is
/// rejected; renaming to the same key is permitted and can replace its material.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminSecretRenameRequest {
    pub to_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminSecretDeleteResponse {
    pub existed: bool,
}

// ===== Libsecret admin CRUD =====
//
// Structured admin surface for the libsecret slot. Mirrors the Secret
// Service spec hierarchy: a Service-level alias table plus a set of
// Collections, each holding Items. Mutations replace a complete resource blob
// atomically, but the load-modify-save sequence is not a transaction and remains
// last-writer-wins under concurrent mutation.
//
// `created` is set on POST, `modified` is bumped server-side on every
// mutation; clients never send either field. Item secret bytes are
// omitted from the tree view and exposed on a dedicated reveal
// endpoint so a bulk listing never carries secret material.

/// Whole-tree view of one resource's libsecret section. The shape
/// the admin SPA renders top-level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretTreeResponse {
    /// Alias name → collection id. Free-form name space at the
    /// Service level; aliases pointing at non-existent collections
    /// are not produced by this endpoint (the alias-PUT handler
    /// enforces target existence).
    pub aliases: BTreeMap<String, String>,
    /// Collections in id order. Item secret bytes are omitted; the
    /// SPA fetches them on demand via the reveal endpoint.
    pub collections: Vec<AdminLibsecretCollectionView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretCollectionView {
    pub id: String,
    pub label: String,
    pub created: u64,
    pub modified: u64,
    pub items: Vec<AdminLibsecretItemView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretItemView {
    pub id: String,
    pub label: String,
    pub content_type: String,
    pub created: u64,
    pub modified: u64,
    pub attributes: BTreeMap<String, String>,
}

/// `GET .../libsecret/collections/{cid}/items/{iid}/secret`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretItemSecretResponse {
    pub secret_b64: String,
}

/// `POST .../libsecret/collections`. `id` is optional; when absent the
/// server derives one from the label.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretCollectionCreateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretCollectionCreateResponse {
    pub id: String,
}

/// `DELETE .../libsecret/collections/{cid}`. The deletion cascades to
/// the collection's items in one save. Any aliases pointing at the
/// deleted collection are scrubbed atomically and their names
/// reported in `aliases_scrubbed`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretCollectionDeleteResponse {
    pub existed: bool,
    #[serde(default)]
    pub aliases_scrubbed: Vec<String>,
}

/// `POST .../libsecret/collections/{cid}/items`. `id` is optional; when
/// absent the server derives one from the label.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretItemCreateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub label: String,
    pub content_type: String,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    pub secret_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretItemCreateResponse {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretItemDeleteResponse {
    pub existed: bool,
}

/// `PUT .../libsecret/aliases/{name}`. `collection_id` must name an
/// existing collection; the handler returns 404 otherwise.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretAliasUpsertRequest {
    pub collection_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminLibsecretAliasDeleteResponse {
    pub existed: bool,
}

// ===== Wrapper permission CRUD =====
//
// One `WrapperPermission` per `(client_instance, wrapper, resource)`
// tuple. The tuple lives in the URL path; the body only carries the
// permission's mutable fields. The mode controls *if* the permission
// is usable; the `writable` flag decides wrapper-side writes for the
// `files`/`libsecret` slots.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminWrapperPermissionsListResponse {
    pub permissions: Vec<WrapperPermission>,
}

/// Body of `PUT /v1/admin/stores/{id}/wrapper-permissions/...`. The
/// `(client_instance, wrapper, resource_id)` tuple comes from the URL
/// path; the body holds the mutable parts of a `WrapperPermission`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminWrapperPermissionUpsertRequest {
    pub mode: crate::policy::WrapperApprovalMode,
    pub writable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminWrapperPermissionKey {
    pub client_instance: String,
    pub wrapper: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminResourcesListResponse {
    pub resources: Vec<ResourceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminResourceCreateRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminResourceCreateResponse {
    pub resource: ResourceRecord,
}

/// Body of `PUT /v1/admin/stores/{id}/resources/{rid}` — rename an
/// existing resource by id. The id stays stable; the name changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminResourceRenameRequest {
    pub new_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminResourceRenameResponse {
    pub resource: ResourceRecord,
}

/// Counts plus a short identifier-only sample per dependent kind. Samples are
/// capped server-side so a heavily-used resource does not return thousands of
/// entries. Identifiers are not copied into audit events or logs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema, Default)]
pub struct AdminResourceDeleteBlockers {
    pub content_entry_count: u32,
    pub sample_content: Vec<ResourceContentReference>,
    pub pair_count: u32,
    #[serde(default)]
    pub sample_pairs: Vec<ServiceWrapperResourcePair>,
    pub permission_count: u32,
    #[serde(default)]
    pub sample_permissions: Vec<AdminWrapperPermissionKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminResourceDeleteResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<AdminResourceDeleteBlockers>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminPairsListResponse {
    pub pairs: Vec<WrapperResourcePairRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminPairCreateRequest {
    pub wrapper: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminPairCreateResponse {
    pub pair: WrapperResourcePairRecord,
}

/// Body of `POST /v1/admin/stores/{id}/pairs/with-new-resource` —
/// create a fresh resource and pair it with `wrapper` in one request.
/// The resource catalogue and pair document are saved separately.
/// A failed pair save triggers best-effort resource rollback; the operation is
/// not transactional, and failed rollback can leave an unpaired resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminPairWithNewResourceRequest {
    pub wrapper: String,
    pub resource_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminPairWithNewResourceResponse {
    pub resource: ResourceRecord,
    pub pair: WrapperResourcePairRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema, Default)]
pub struct AdminPairDeleteBlockers {
    pub permission_count: u32,
    #[serde(default)]
    pub sample_permissions: Vec<AdminWrapperPermissionKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminPairDeleteResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<AdminPairDeleteBlockers>,
}

/// One `(wrapper, resource)` pair on the admin wire. Used in
/// resource-delete blocker payloads to name the dependent pairs that
/// would have to be cleaned up before the delete can proceed. The
/// persisted record (`WrapperResourcePairRecord`) adds provenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct ServiceWrapperResourcePair {
    pub wrapper: String,
    pub resource: String,
}

// ===== Aggregate views =====
//
// One-shot reads that fold the three documents a busy SPA page needs
// into a single round trip. Pure server-side joins of documents
// already in the store; no new authorisation surface.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminWrapperPermissionsView {
    pub permissions: Vec<WrapperPermission>,
    pub pairs: Vec<WrapperResourcePairRecord>,
    pub resources: Vec<ResourceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct AdminSecretsView {
    pub entries: Vec<AdminSecretEntry>,
    pub resources: Vec<ResourceRecord>,
}
