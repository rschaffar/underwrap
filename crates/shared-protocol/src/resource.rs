//! Resource naming helpers.
//!
//! A backend resource is stored as one canonical-JSON blob keyed by
//! `resources/<resource_id>` (see [`crate::resource_blob`]). The blob
//! carries three logical slots inside it:
//!
//! - `env/<VAR>`
//! - `files/<relative-path>`
//! - `libsecret/<relative-id>`
//!
//! Policy is slot-scoped via `env:<resource>`, `files:<resource>`, and
//! `libsecret:<resource>` — the scoping strings here describe the grant
//! key, not a storage path.
//!
//! Admin endpoints address per-slot entries via the structured tuple
//! `(resource, slot, key)`; [`compose_secret_id`] is the canonical
//! mapping from that tuple to the in-blob entry key used by the
//! resource bundle expansion. [`parse_secret_id`] is the inverse, kept
//! around for code paths that still receive a flattened id from the
//! wire (e.g. legacy fixtures).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const ENV_SLOT: &str = "env";
pub const FILES_SLOT: &str = "files";
pub const LIBSECRET_SLOT: &str = "libsecret";

/// Storage key for the canonical resource blob.
pub fn resource_blob_key(resource_id: &str) -> String {
    format!("resources/{resource_id}")
}

pub fn env_resource(resource_id: &str) -> String {
    format!("env:{resource_id}")
}

pub fn files_resource(resource_id: &str) -> String {
    format!("files:{resource_id}")
}

pub fn libsecret_resource(resource_id: &str) -> String {
    format!("libsecret:{resource_id}")
}

/// The three logical slots inside a resource blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export)]
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
/// `__underwrap/*` system ids the daemon doesn't gate on.
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

/// Convenience: just the resource id from a fully-qualified secret id.
pub fn resource_from_secret_id(secret_id: &str) -> Option<&str> {
    parse_secret_id(secret_id).map(|p| p.resource_id)
}

/// Convenience helpers that build the per-slot key shape used by the
/// per-key admin API and by the in-memory per-resource bundle entries
/// that secretsd / directory channels consume. The on-disk layout is
/// one blob per resource (see [`resource_blob_key`]); these helpers
/// only describe the address space of legacy in-memory keys.
pub fn env_prefix(resource_id: &str) -> String {
    format!("{resource_id}/{ENV_SLOT}")
}

pub fn files_prefix(resource_id: &str) -> String {
    format!("{resource_id}/{FILES_SLOT}")
}

pub fn libsecret_prefix(resource_id: &str) -> String {
    format!("{resource_id}/{LIBSECRET_SLOT}")
}

pub fn relative_to_prefix<'a>(id: &'a str, prefix: &str) -> Option<&'a str> {
    id.strip_prefix(prefix)?.strip_prefix('/')
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
mod tests {
    use super::*;

    #[test]
    fn resource_blob_key_is_stable() {
        assert_eq!(resource_blob_key("github/token"), "resources/github/token");
        assert_eq!(resource_blob_key("gh"), "resources/gh");
    }

    #[test]
    fn parse_secret_id_handles_all_slots() {
        let p = parse_secret_id("github/token/env/GH_TOKEN").unwrap();
        assert_eq!(p.resource_id, "github/token");
        assert_eq!(p.slot, ResourceSlot::Env);
        assert_eq!(p.inner, "GH_TOKEN");

        let p = parse_secret_id("gws/state/files/config.yaml").unwrap();
        assert_eq!(p.resource_id, "gws/state");
        assert_eq!(p.slot, ResourceSlot::Files);
        assert_eq!(p.inner, "config.yaml");

        let p = parse_secret_id("gh/libsecret/oauth").unwrap();
        assert_eq!(p.resource_id, "gh");
        assert_eq!(p.slot, ResourceSlot::Libsecret);
        assert_eq!(p.inner, "oauth");

        // System keys (no slot in the canonical position) → None.
        assert!(parse_secret_id("__underwrap/policy/document").is_none());
        assert!(parse_secret_id("no-slot-here").is_none());
    }

    #[test]
    fn resource_from_secret_id_returns_resource_only() {
        assert_eq!(
            resource_from_secret_id("github/token/env/GH_TOKEN"),
            Some("github/token"),
        );
        assert_eq!(resource_from_secret_id("__underwrap/policy/document"), None);
    }

    #[test]
    fn compose_secret_id_round_trips_through_parse() {
        let composed = compose_secret_id("github/token", ResourceSlot::Env, "GH_TOKEN");
        assert_eq!(composed, "github/token/env/GH_TOKEN");
        let parsed = parse_secret_id(&composed).unwrap();
        assert_eq!(parsed.resource_id, "github/token");
        assert_eq!(parsed.slot, ResourceSlot::Env);
        assert_eq!(parsed.inner, "GH_TOKEN");
    }

    #[test]
    fn validate_secret_key_accepts_per_slot_shapes() {
        assert!(validate_secret_key(ResourceSlot::Env, "GH_TOKEN").is_ok());
        assert!(validate_secret_key(ResourceSlot::Files, "config.yaml").is_ok());
        assert!(validate_secret_key(ResourceSlot::Files, "nested/dir/file").is_ok());
        assert!(validate_secret_key(ResourceSlot::Libsecret, "default/gh-cli.secret").is_ok());
    }

    #[test]
    fn validate_secret_key_rejects_invalid_shapes() {
        assert!(validate_secret_key(ResourceSlot::Env, "").is_err());
        assert!(validate_secret_key(ResourceSlot::Env, "bad-name").is_err());
        assert!(validate_secret_key(ResourceSlot::Env, "PATH").is_err());
        assert!(validate_secret_key(ResourceSlot::Files, "").is_err());
        assert!(validate_secret_key(ResourceSlot::Files, "/abs").is_err());
        assert!(validate_secret_key(ResourceSlot::Files, "a/../b").is_err());
        assert!(validate_secret_key(ResourceSlot::Files, "a//b").is_err());
        assert!(validate_secret_key(ResourceSlot::Libsecret, "x\0y").is_err());
    }

    #[test]
    fn env_validation_rejects_dangerous_names() {
        assert!(validate_env_name("GH_TOKEN").is_ok());
        assert!(validate_env_name("_TOKEN1").is_ok());
        assert!(validate_env_name("LD_PRELOAD").is_err());
        assert!(validate_env_name("PATH").is_err());
        assert!(validate_env_name("UNDERWRAP_SECRETSD").is_err());
        assert!(validate_env_name("1TOKEN").is_err());
        assert!(validate_env_name("BAD-NAME").is_err());
    }
}
