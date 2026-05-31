//! Runtime API DTOs — the daemon ↔ backend (`underwrap-backendd`) wire,
//! authenticated by mTLS. Resource-vocabulary surface: the runtime
//! client addresses wrappers and resources by **name** only. There is
//! no `secret_id` field anywhere here and no way to address `control/*`
//! or `<resource>/<slot>/<key>` storage keys. The backend re-derives the
//! principal from the mTLS cert and re-evaluates the policy document on
//! every call; there are no sessions or open-call state between open and
//! finalize.

use serde::{Deserialize, Serialize};

/// Canonical, stable reason discriminators for a runtime invocation
/// `Deny`. These variants are the **single source of truth** for the
/// kebab-case strings emitted in `RuntimeWrapperInvocation*Response::Deny`
/// — the emitters (`policy::decide`, the invocation handlers) construct
/// them through this enum so the wire vocabulary cannot drift.
///
/// The `Deny.reason` field stays a plain `String` on the wire because it
/// is **human-facing**: for most denials it is exactly one of these
/// discriminators, but a `Locked` permission carrying an operator-supplied
/// reason surfaces that free text verbatim *instead of* `locked` (it
/// reaches the end user as the wrapper's error message). The daemon does
/// not branch on it today; it forwards it as the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeDenyReason {
    /// No permission row matched `(client_instance, wrapper, resource)`.
    NoPermission,
    /// The `(wrapper, resource)` pair is not registered in this store.
    UnregisteredPair,
    /// The matched permission is `Locked` and carries no operator reason.
    Locked,
    /// The matched permission's TTL has elapsed.
    Expired,
    /// A `Debug` variant against a non-`UnsafeDebugUntil` permission.
    NotUnsafeDebug,
    /// A writeback against a permission whose `writable` flag is false.
    NotWritable,
}

impl RuntimeDenyReason {
    /// The stable kebab-case wire string for this reason.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoPermission => "no-permission",
            Self::UnregisteredPair => "unregistered-pair",
            Self::Locked => "locked",
            Self::Expired => "expired",
            Self::NotUnsafeDebug => "not-unsafe-debug",
            Self::NotWritable => "not-writable",
        }
    }
}

impl std::fmt::Display for RuntimeDenyReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Canonical, stable reason discriminators for a finalize `InvalidBlob`
/// rejection — writeback validation failures that are a client bug and
/// won't change on retry (unlike a policy `Deny`). Single source of truth
/// for these wire strings, same rationale as [`RuntimeDenyReason`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeInvalidBlobReason {
    /// The base64 envelope failed to decode.
    DecodeFailed,
    /// The blob's `schema_version` is not one this backend supports.
    SchemaUnknown,
    /// The decoded blob is not byte-for-byte JCS-canonical.
    NoncanonicalBlob,
    /// The writeback tried to change the immutable `env` section.
    EnvSectionImmutable,
}

impl RuntimeInvalidBlobReason {
    /// The stable kebab-case wire string for this reason.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DecodeFailed => "decode-failed",
            Self::SchemaUnknown => "schema-unknown",
            Self::NoncanonicalBlob => "noncanonical-blob",
            Self::EnvSectionImmutable => "env-section-immutable",
        }
    }
}

impl std::fmt::Display for RuntimeInvalidBlobReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Stable `ServiceErrorResponse.code` value the runtime API emits when a
/// request lands on a locked (encrypted, no master key) store. Single
/// source of truth shared by the backend emitter
/// (`runtime_api::error::RuntimeError::store_locked`) and the daemon's
/// error classifier (`frontend-underwrapd`'s `parse_error`) so the two
/// sides cannot drift on the magic string.
pub const ERROR_CODE_STORE_LOCKED: &str = "store_locked";

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
    /// Real target or debug tool. The backend evaluates the policy for
    /// this variant; only `UnsafeDebugUntil` admits `Debug`.
    #[serde(default)]
    pub variant: crate::variant::WrapperVariant,
}

/// Response to `POST /v1/wrapper-invocations`.
///
/// `Deny.reason` is a human-facing string. For the canonical denials it
/// is exactly one of the [`RuntimeDenyReason`] kebab-case discriminators
/// (`no-permission`, `unregistered-pair`, `locked`, `expired`,
/// `not-unsafe-debug`); for a `Locked` permission with an operator
/// reason, that free text replaces `locked` and is shown to the user.
/// `not-writable` is finalize-only and never appears here. The daemon
/// forwards the string as the wrapper error message rather than
/// branching on it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum RuntimeWrapperInvocationOpenResponse {
    Allow {
        /// Stable backend resource id (e.g. `res_<hex>`). Opaque to the
        /// daemon — it never addresses storage by it — but not unused:
        /// the daemon relays it to the wrapper inside
        /// `WrapperResponse::ReadOk`, and `frontend-ns` surfaces it in
        /// audit events and the user-facing writeback-rejected message.
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
    /// Real target or debug tool — re-evaluated server-side on finalize.
    #[serde(default)]
    pub variant: crate::variant::WrapperVariant,
    /// `Some` = writeback. The wrapper sends the entire updated blob,
    /// not a diff. `None` = usage signal only (still bumps last_used_at).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_b64: Option<String>,
}

/// Response to `POST /v1/wrapper-invocations/finalize`.
///
/// `Deny` covers policy denials (`Deny.reason` is a [`RuntimeDenyReason`]
/// discriminator — `not-writable` in addition to the open-time set, or a
/// `Locked` operator reason). `InvalidBlob` covers writeback validation
/// failures (`InvalidBlob.reason` is a [`RuntimeInvalidBlobReason`]
/// discriminator: `decode-failed`, `schema-unknown`, `noncanonical-blob`,
/// `env-section-immutable`). They are split apart because a `Deny` may
/// flip to `Allow` after an operator edits the policy, whereas
/// `InvalidBlob` is a client bug that won't change on retry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum RuntimeWrapperInvocationFinalizeResponse {
    Ok,
    Deny { reason: String },
    InvalidBlob { reason: String },
}
