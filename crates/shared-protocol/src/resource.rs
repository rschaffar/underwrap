//! Resource naming helpers.
//!
//! A backend resource is stored as one canonical-JSON blob keyed by
//! `resources/<resource_id>` (see [`crate::resource_blob`]). The blob
//! carries three logical slots inside it:
//!
//! - `env/<VAR>`
//! - `files/<relative-path>`
//! - `libsecret/...` (a structured subtree, addressed by its own
//!   admin endpoints rather than per-key)
//!
//! Admin endpoints for the flat `env` / `files` slots address per-key
//! entries via the structured tuple `(resource, slot, key)`;
//! [`compose_secret_id`] is the canonical mapping from that tuple to
//! the in-blob entry key, and [`parse_secret_id`] is the inverse used
//! by the storage helpers in `secret_entries.rs`.

use serde::{Deserialize, Serialize};

const ENV_SLOT: &str = "env";
const FILES_SLOT: &str = "files";
const LIBSECRET_SLOT: &str = "libsecret";

/// Storage key for the canonical resource blob.
pub fn resource_blob_key(resource_id: &str) -> String {
    format!("resources/{resource_id}")
}

/// The three logical slots inside a resource blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ResourceSlot {
    Env,
    Files,
    Libsecret,
}

impl ResourceSlot {
    pub fn name(self) -> &'static str {
        match self {
            Self::Env => ENV_SLOT,
            Self::Files => FILES_SLOT,
            Self::Libsecret => LIBSECRET_SLOT,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            ENV_SLOT => Some(Self::Env),
            FILES_SLOT => Some(Self::Files),
            LIBSECRET_SLOT => Some(Self::Libsecret),
            _ => None,
        }
    }
}

impl std::str::FromStr for ResourceSlot {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or(())
    }
}

/// Subset of [`ResourceSlot`] that the flat-KV admin surface emits.
/// The `libsecret` slot is structurally a tree and is addressed by
/// its own admin endpoints, so it never appears in flat-entry listings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum KvSecretSlot {
    Env,
    Files,
}

impl From<KvSecretSlot> for ResourceSlot {
    fn from(s: KvSecretSlot) -> Self {
        match s {
            KvSecretSlot::Env => Self::Env,
            KvSecretSlot::Files => Self::Files,
        }
    }
}

impl TryFrom<ResourceSlot> for KvSecretSlot {
    type Error = ResourceSlot;
    fn try_from(s: ResourceSlot) -> Result<Self, Self::Error> {
        match s {
            ResourceSlot::Env => Ok(Self::Env),
            ResourceSlot::Files => Ok(Self::Files),
            ResourceSlot::Libsecret => Err(s),
        }
    }
}

/// Compose the in-blob entry key for `(resource_id, slot, key)`. This
/// is the canonical mapping admin handlers use to route a structured
/// secret address into the legacy entry map produced by
/// `expand_blob_to_entries`.
pub fn compose_secret_id(resource_id: &str, slot: ResourceSlot, key: &str) -> String {
    format!("{resource_id}/{}/{key}", slot.name())
}

/// Validate a per-slot key supplied by an operator. Returns the
/// trimmed key on success and a user-facing error string on rejection.
///
/// Rules:
/// - `env`: must satisfy [`validate_env_name`] (identifier-shaped and
///   not a reserved hardening name).
/// - `files`, `libsecret`: path-shaped — non-empty, no leading slash,
///   no `.`/`..`/empty segments, no null bytes.
pub fn validate_secret_key(slot: ResourceSlot, key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("key must not be empty".into());
    }
    match slot {
        ResourceSlot::Env => validate_env_name(key).map_err(str::to_string),
        ResourceSlot::Files | ResourceSlot::Libsecret => {
            if has_traversal(key) {
                return Err(
                    "key must be a relative path with no empty segments, '.' or '..'".into(),
                );
            }
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSecretId<'a> {
    pub resource_id: &'a str,
    pub slot: ResourceSlot,
    pub inner: &'a str,
}

/// Parse a secret id like `<resource>/env/<VAR>`, `<resource>/files/<path>`,
/// or `<resource>/libsecret/<id>` into resource + slot + inner. Returns
/// `None` for ids that don't match — including the reserved
/// `control/*` system ids the daemon doesn't gate on.
pub fn parse_secret_id(secret_id: &str) -> Option<ParsedSecretId<'_>> {
    let mut best: Option<(ResourceSlot, usize)> = None;
    for slot in [
        ResourceSlot::Env,
        ResourceSlot::Files,
        ResourceSlot::Libsecret,
    ] {
        let needle = format!("/{}/", slot.name());
        if let Some(idx) = secret_id.find(&needle)
            && best.map(|(_, b)| idx < b).unwrap_or(true)
        {
            best = Some((slot, idx));
        }
    }
    let (slot, cut) = best?;
    if cut == 0 {
        return None;
    }
    let resource_id = &secret_id[..cut];
    let inner_start = cut + 1 + slot.name().len() + 1;
    if inner_start > secret_id.len() {
        return None;
    }
    Some(ParsedSecretId {
        resource_id,
        slot,
        inner: &secret_id[inner_start..],
    })
}

pub fn has_traversal(path: &str) -> bool {
    path.is_empty()
        || path.starts_with('/')
        || path.contains('\0')
        || path
            .split('/')
            .any(|segment| segment == "." || segment == ".." || segment.is_empty())
}

pub fn validate_env_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("environment variable name is empty");
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("environment variable name is empty");
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return Err("environment variable name must start with '_' or an ASCII letter");
    }
    if !chars.all(|c| c == '_' || c.is_ascii_alphanumeric()) {
        return Err("environment variable name contains invalid characters");
    }
    if is_reserved_env_name(name) {
        return Err("environment variable name is reserved by underwrap hardening");
    }
    Ok(())
}

pub fn is_reserved_env_name(name: &str) -> bool {
    name.starts_with("LD_")
        || matches!(
            name,
            "LD_PRELOAD"
                | "LD_LIBRARY_PATH"
                | "LD_AUDIT"
                | "PATH"
                | "HOME"
                | "USER"
                | "LOGNAME"
                | "SHELL"
                | "DBUS_SESSION_BUS_ADDRESS"
                | "SSH_AUTH_SOCK"
        )
        || name.starts_with("UNDERWRAP_")
}

#[cfg(test)]
mod tests;
