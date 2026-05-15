//! Operator records: the people who can log in to the admin UI.
//!
//! Records live as a single JSON blob under the reserved secret id
//! [`OPERATORS_SECRET_ID`] in the configured admin-home store. Subsequent
//! tickets (211 login, 213 role middleware, 214 bootstrap, 240 user
//! management) consume this module.
//!
//! Password hashes are Argon2id PHC strings. We never log them; the
//! [`OperatorRecord`] `Debug` impl redacts them and tests assert that
//! tracing-captured CRUD output never contains the hash material.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use underwrap_store::StorageBackend;

/// Reserved secret id where the operator collection lives.
pub const OPERATORS_SECRET_ID: &str = "__underwrap/admin/operators";

/// Schema version for the persisted operator collection. Bump when the
/// on-disk shape changes incompatibly.
pub const COLLECTION_VERSION: u32 = 1;

/// One operator account.
///
/// `password_hash` is an Argon2id PHC string. It must never appear in
/// logs; the custom `Debug` impl below redacts it.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorRecord {
    /// Stable identifier. Random 16-byte hex string, generated server-side.
    pub id: String,
    /// Lowercase, trimmed. Unique across the collection.
    pub username: String,
    /// Argon2id PHC-encoded password hash.
    pub password_hash: String,
    pub role: OperatorRole,
    #[serde(default)]
    pub granted_stores: Vec<String>,
    pub created_at: u64,
    pub password_changed_at: u64,
}

impl std::fmt::Debug for OperatorRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperatorRecord")
            .field("id", &self.id)
            .field("username", &self.username)
            .field("password_hash", &"<redacted>")
            .field("role", &self.role)
            .field("granted_stores", &self.granted_stores)
            .field("created_at", &self.created_at)
            .field("password_changed_at", &self.password_changed_at)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OperatorRole {
    Admin,
    Operator,
}

/// Resolved identity and authorization context for an admin request.
/// Derived from either a session cookie (looked up in the operator
/// collection at request time) or a legacy bearer token. Carries
/// enough information to gate per-store access without re-reading the
/// collection on every authorization decision.
#[derive(Debug, Clone)]
pub struct Principal {
    pub id: String,
    pub username: String,
    pub role: OperatorRole,
    pub granted_stores: Vec<String>,
}

impl Principal {
    pub fn is_admin(&self) -> bool {
        matches!(self.role, OperatorRole::Admin)
    }

    /// Admins can access any store; operators must have an explicit
    /// grant. We don't distinguish "no grant" from "no such store"
    /// at the response level — both return 404 to avoid leaking
    /// store existence to non-admins.
    pub fn has_store_access(&self, store: &str) -> bool {
        self.is_admin() || self.granted_stores.iter().any(|s| s == store)
    }
}

/// The persisted operator collection. Stored as JSON in the admin-home
/// store under [`OPERATORS_SECRET_ID`].
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorCollection {
    pub version: u32,
    pub operators: Vec<OperatorRecord>,
}

impl std::fmt::Debug for OperatorCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `OperatorRecord`'s Debug impl already redacts the hash; we
        // implement Debug here to keep the same redaction-by-default
        // posture for the wrapper.
        f.debug_struct("OperatorCollection")
            .field("version", &self.version)
            .field("operators", &self.operators)
            .finish()
    }
}

impl Default for OperatorCollection {
    fn default() -> Self {
        Self {
            version: COLLECTION_VERSION,
            operators: Vec::new(),
        }
    }
}

impl OperatorCollection {
    /// Number of operator records currently stored.
    pub fn len(&self) -> usize {
        self.operators.len()
    }

    /// True when no operator records exist. Used by the bootstrap flow
    /// in ticket 214 to decide whether to show the bootstrap page.
    pub fn is_empty(&self) -> bool {
        self.operators.is_empty()
    }

    /// Look up an operator by username. Username comparison is case-
    /// insensitive after the per-record `username` has been normalised
    /// on insert.
    pub fn find_by_username(&self, username: &str) -> Option<&OperatorRecord> {
        let needle = normalise_username(username);
        self.operators.iter().find(|o| o.username == needle)
    }

    /// Append a record. Refuses duplicate usernames.
    pub fn add(&mut self, mut record: OperatorRecord) -> Result<()> {
        record.username = normalise_username(&record.username);
        if record.username.is_empty() {
            anyhow::bail!("operator username must be non-empty");
        }
        if !record.password_hash.starts_with("$argon2") {
            anyhow::bail!("operator password_hash must be a PHC-encoded argon2 hash");
        }
        if self.find_by_username(&record.username).is_some() {
            anyhow::bail!("operator '{}' already exists", record.username);
        }
        self.operators.push(record);
        Ok(())
    }
}

/// Normalise a username: trimmed, lowercase. Usernames are compared
/// case-insensitively because operators routinely confuse `Alice` and
/// `alice` between admin set-up and login.
pub fn normalise_username(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Generate a new opaque operator id. 16 random bytes hex-encoded, so
/// it looks UUID-shaped without pulling in the `uuid` crate.
pub fn new_operator_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Current epoch seconds, saturating to 0 on the very-unlikely
/// pre-1970 clock case.
pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Load the operator collection from the admin-home store. Returns
/// the default (empty) collection when no record exists yet; the
/// caller treats that as "bootstrap required".
pub fn load(store: &dyn StorageBackend) -> Result<OperatorCollection> {
    let raw = store
        .read(OPERATORS_SECRET_ID)
        .context("reading operator collection from admin-home store")?;
    let Some(bytes) = raw else {
        return Ok(OperatorCollection::default());
    };
    let parsed: OperatorCollection =
        serde_json::from_slice(bytes.as_ref()).context("parsing operator collection JSON")?;
    if parsed.version != COLLECTION_VERSION {
        anyhow::bail!(
            "operator collection has version {}, expected {}",
            parsed.version,
            COLLECTION_VERSION
        );
    }
    Ok(parsed)
}

/// Persist the operator collection. Atomicity is the storage backend's
/// responsibility (plain/encrypted both write via a tempfile + rename).
pub fn save(store: &dyn StorageBackend, collection: &OperatorCollection) -> Result<()> {
    let bytes =
        serde_json::to_vec(collection).context("serialising operator collection to JSON")?;
    store
        .write(OPERATORS_SECRET_ID, &bytes)
        .context("writing operator collection to admin-home store")
}

/// Resolve which store should hold the operator collection. Defaults
/// to the alphabetically first declared store when no explicit
/// `admin_home_store` is configured.
pub fn resolve_admin_home_store<'a>(
    stores: &'a std::collections::HashMap<String, std::sync::Arc<dyn StorageBackend>>,
    configured: Option<&'a str>,
) -> Result<(&'a str, std::sync::Arc<dyn StorageBackend>)> {
    if let Some(name) = configured {
        let store = stores
            .get(name)
            .with_context(|| format!("admin_home_store '{name}' is not declared in stores"))?;
        return Ok((name, store.clone()));
    }
    let mut names: Vec<&str> = stores.keys().map(|s| s.as_str()).collect();
    names.sort();
    let first = names
        .first()
        .copied()
        .context("no stores declared; cannot resolve admin_home_store")?;
    Ok((first, stores.get(first).unwrap().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use underwrap_protocol::message::SecretBytes;

    struct MemStore {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemStore {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                data: Mutex::new(HashMap::new()),
            })
        }
    }

    impl StorageBackend for MemStore {
        fn read(&self, id: &str) -> Result<Option<SecretBytes>> {
            Ok(self.data.lock().unwrap().get(id).cloned().map(Into::into))
        }
        fn write(&self, id: &str, m: &[u8]) -> Result<()> {
            self.data.lock().unwrap().insert(id.to_string(), m.to_vec());
            Ok(())
        }
        fn list(&self, prefix: &str) -> Result<Vec<String>> {
            Ok(self
                .data
                .lock()
                .unwrap()
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }
        fn delete(&self, id: &str) -> Result<bool> {
            Ok(self.data.lock().unwrap().remove(id).is_some())
        }
    }

    fn sample_record(username: &str, role: OperatorRole) -> OperatorRecord {
        OperatorRecord {
            id: new_operator_id(),
            username: username.to_string(),
            password_hash:
                "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
                    .to_string(),
            role,
            granted_stores: Vec::new(),
            created_at: 1000,
            password_changed_at: 1000,
        }
    }

    #[test]
    fn collection_round_trips_through_serde() {
        let mut c = OperatorCollection::default();
        c.add(sample_record("alice", OperatorRole::Admin)).unwrap();
        c.add(sample_record("bob", OperatorRole::Operator)).unwrap();
        let bytes = serde_json::to_vec(&c).unwrap();
        let parsed: OperatorCollection = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn add_rejects_duplicate_username_case_insensitively() {
        let mut c = OperatorCollection::default();
        c.add(sample_record("Alice", OperatorRole::Admin)).unwrap();
        let err = c
            .add(sample_record("alice", OperatorRole::Operator))
            .unwrap_err();
        assert!(err.to_string().contains("already exists"));
        // Username has been normalised on insert.
        assert_eq!(c.operators[0].username, "alice");
    }

    #[test]
    fn add_rejects_empty_username() {
        let mut c = OperatorCollection::default();
        let err = c
            .add(sample_record("   ", OperatorRole::Admin))
            .unwrap_err();
        assert!(err.to_string().contains("non-empty"));
    }

    #[test]
    fn add_rejects_non_argon2_hash() {
        let mut c = OperatorCollection::default();
        let mut bad = sample_record("alice", OperatorRole::Admin);
        bad.password_hash = "plaintext".to_string();
        let err = c.add(bad).unwrap_err();
        assert!(err.to_string().contains("argon2"));
    }

    #[test]
    fn find_by_username_is_case_insensitive() {
        let mut c = OperatorCollection::default();
        c.add(sample_record("Alice", OperatorRole::Admin)).unwrap();
        assert!(c.find_by_username("alice").is_some());
        assert!(c.find_by_username("ALICE").is_some());
        assert!(c.find_by_username("  alice  ").is_some());
        assert!(c.find_by_username("bob").is_none());
    }

    #[test]
    fn empty_collection_signals_bootstrap_required() {
        let store = MemStore::new();
        let collection = load(store.as_ref()).unwrap();
        assert!(collection.is_empty(), "fresh store yields empty collection");
    }

    #[test]
    fn save_and_load_round_trip() {
        let store = MemStore::new();
        let mut c = OperatorCollection::default();
        c.add(sample_record("alice", OperatorRole::Admin)).unwrap();
        save(store.as_ref(), &c).unwrap();
        let loaded = load(store.as_ref()).unwrap();
        assert_eq!(loaded, c);
    }

    #[test]
    fn load_rejects_unknown_version() {
        let store = MemStore::new();
        let bad = r#"{"version":99,"operators":[]}"#;
        store.write(OPERATORS_SECRET_ID, bad.as_bytes()).unwrap();
        let err = load(store.as_ref()).unwrap_err();
        assert!(err.to_string().contains("version"));
    }

    #[test]
    fn resolve_admin_home_store_picks_first_alphabetically_by_default() {
        let s_b: Arc<dyn StorageBackend> = MemStore::new();
        let s_a: Arc<dyn StorageBackend> = MemStore::new();
        let mut stores: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        stores.insert("b".to_string(), s_b);
        stores.insert("a".to_string(), s_a);
        let (name, _) = resolve_admin_home_store(&stores, None).unwrap();
        assert_eq!(name, "a");
    }

    #[test]
    fn resolve_admin_home_store_uses_configured_name() {
        let s_b: Arc<dyn StorageBackend> = MemStore::new();
        let s_a: Arc<dyn StorageBackend> = MemStore::new();
        let mut stores: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        stores.insert("b".to_string(), s_b);
        stores.insert("a".to_string(), s_a);
        let (name, _) = resolve_admin_home_store(&stores, Some("b")).unwrap();
        assert_eq!(name, "b");
    }

    #[test]
    fn resolve_admin_home_store_rejects_unknown_configured_name() {
        let stores: HashMap<String, Arc<dyn StorageBackend>> = HashMap::new();
        // unwrap_err() needs Result<T: Debug>; the success tuple holds a
        // trait object, so unwrap the error variant by hand instead.
        let result = resolve_admin_home_store(&stores, Some("nope"));
        let err = match result {
            Ok(_) => panic!("expected an error for unknown configured store"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("not declared"));
    }

    #[test]
    fn principal_admin_has_access_to_every_store() {
        let p = Principal {
            id: "a".into(),
            username: "alice".into(),
            role: OperatorRole::Admin,
            granted_stores: Vec::new(),
        };
        assert!(p.is_admin());
        assert!(p.has_store_access("anything"));
        assert!(p.has_store_access("else"));
    }

    #[test]
    fn principal_operator_requires_explicit_grant() {
        let p = Principal {
            id: "b".into(),
            username: "bob".into(),
            role: OperatorRole::Operator,
            granted_stores: vec!["work".into()],
        };
        assert!(!p.is_admin());
        assert!(p.has_store_access("work"));
        assert!(!p.has_store_access("other"));
    }

    #[test]
    fn debug_redacts_password_hash() {
        let r = sample_record("alice", OperatorRole::Admin);
        let dbg = format!("{r:?}");
        assert!(
            !dbg.contains("argon2"),
            "Debug must not leak the PHC string"
        );
        assert!(dbg.contains("<redacted>"));
        // Same applies inside a collection.
        let mut c = OperatorCollection::default();
        c.add(r).unwrap();
        let dbg = format!("{c:?}");
        assert!(!dbg.contains("argon2"));
    }
}
