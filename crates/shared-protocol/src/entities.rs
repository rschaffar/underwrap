//! First-class entity records for resources and wrapper/resource pairs.
//!
//! Before this module the only persisted "resource universe" was the
//! catalog blob written by runtime clients on startup, plus whatever
//! string keys happened to appear inside `WrapperPermission`s. That
//! left resources and `(wrapper, resource)` pairs without provenance
//! and made the catalog blob the de-facto source of truth even though
//! nothing in the design said it had to be.
//!
//! These two documents replace the catalog: one persisted list per
//! store, each entry stamped with `created_by` + `created_at` on
//! first creation. Provenance fields are write-once.
//!
//! Storage shape mirrors `PolicyDocument` (see `policy.rs`): a JSON
//! document keyed at a reserved `__underwrap/...` secret id, with a
//! schema version tag so deserialisation can reject unknown versions
//! loudly.
//!
//! The wrapper-permission model (`WrapperPermission`, keyed on
//! `(client_instance, wrapper, resource)`) is intentionally untouched
//! — it still references wrappers and resources by name, and now
//! those names are anchored to records here.
//!
//! `RESERVED_PREFIX` filtering in the admin SPA already hides
//! `__underwrap/...` secret ids from operators, so these documents
//! never appear in the secret browser even though they live in the
//! same store backend.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Reserved secret id used to persist the [`ResourcesDocument`].
pub const RESOURCES_DOCUMENT_SECRET_ID: &str = "__underwrap/resources/document";
/// Reserved secret id used to persist the [`PairsDocument`].
pub const PAIRS_DOCUMENT_SECRET_ID: &str = "__underwrap/pairs/document";

/// Current schema version for both entity documents. Deserialisation
/// rejects any other value rather than silently coercing.
pub const ENTITY_SCHEMA_VERSION: u32 = 1;

/// Who created an entity record. Distinguishes admin operators (who
/// register resources/pairs by hand through the admin API) from
/// runtime client instances (which auto-create on startup when their
/// `auto_create_wrapper_assets` flag is on).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EntityCreator {
    /// An admin operator created this entity through the admin API.
    Operator {
        /// The operator's stable id at creation time.
        id: String,
        /// The operator's username at creation time. Kept as a
        /// denormalised copy so the field stays meaningful even if
        /// the operator is later renamed or deleted.
        username: String,
    },
    /// A runtime client instance auto-created this entity via the
    /// runtime register endpoint.
    ClientInstance {
        /// The instance's stable id at creation time.
        id: String,
    },
}

/// A resource record, one per store.
///
/// `id` is a stable, opaque, write-once identifier (`res_<hex>`). It
/// addresses the resource on disk (`resources/<id>` blob), in pair
/// records, and in wrapper permissions — so renaming a resource
/// preserves every existing reference.
///
/// `name` is the operator-facing label. Mutable via the admin rename
/// endpoint; what wrapper configs reference (operator-friendly); what
/// the SPA shows. Duplicates rejected on insert and on rename.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct ResourceRecord {
    pub id: String,
    pub name: String,
    pub created_by: EntityCreator,
    pub created_at: u64,
}

/// A `(wrapper, resource)` pair record, one per store.
///
/// `resource_id` references a [`ResourceRecord`] by its stable id;
/// the admin create endpoint refuses to create a pair whose resource
/// is not already present, and resource rename does not invalidate
/// the reference.
///
/// `last_used_at` is bumped server-side on every successful runtime
/// open and finalize against the pair. `None` means the pair has
/// never been used since the record was created.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct WrapperResourcePairRecord {
    pub wrapper: String,
    pub resource_id: String,
    pub created_by: EntityCreator,
    pub created_at: u64,
    #[serde(default)]
    pub last_used_at: Option<u64>,
}

/// Store-backed resources document. Missing-document-means-empty
/// semantics (see [`ResourcesDocument::default`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct ResourcesDocument {
    pub version: u32,
    #[serde(default)]
    pub resources: Vec<ResourceRecord>,
}

impl Default for ResourcesDocument {
    fn default() -> Self {
        Self {
            version: ENTITY_SCHEMA_VERSION,
            resources: Vec::new(),
        }
    }
}

/// Store-backed pairs document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct PairsDocument {
    pub version: u32,
    #[serde(default)]
    pub pairs: Vec<WrapperResourcePairRecord>,
}

impl Default for PairsDocument {
    fn default() -> Self {
        Self {
            version: ENTITY_SCHEMA_VERSION,
            pairs: Vec::new(),
        }
    }
}

/// Reason why an entity insert was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertError {
    /// A record with the same primary key already exists.
    Duplicate,
}

impl std::fmt::Display for InsertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InsertError::Duplicate => f.write_str("duplicate entity key"),
        }
    }
}

impl std::error::Error for InsertError {}

impl ResourcesDocument {
    /// Look up by stable id.
    pub fn find_by_id(&self, id: &str) -> Option<&ResourceRecord> {
        self.resources.iter().find(|r| r.id == id)
    }

    /// Look up by current name. Useful for resolving operator-facing
    /// labels (wrapper configs, admin UI text) to a stable id.
    pub fn find_by_name(&self, name: &str) -> Option<&ResourceRecord> {
        self.resources.iter().find(|r| r.name == name)
    }

    /// Insert, refusing duplicates on `id` and on `name`. Existing
    /// records are never replaced — provenance fields are write-once.
    pub fn insert(&mut self, record: ResourceRecord) -> Result<(), InsertError> {
        if self.find_by_id(&record.id).is_some() || self.find_by_name(&record.name).is_some() {
            return Err(InsertError::Duplicate);
        }
        self.resources.push(record);
        Ok(())
    }

    /// Rename an existing resource by id. Returns `true` if a record
    /// was renamed; `Err(Duplicate)` if `new_name` is already taken
    /// by another resource.
    pub fn rename(&mut self, id: &str, new_name: &str) -> Result<bool, InsertError> {
        if self
            .resources
            .iter()
            .any(|r| r.id != id && r.name == new_name)
        {
            return Err(InsertError::Duplicate);
        }
        for r in &mut self.resources {
            if r.id == id {
                r.name = new_name.to_string();
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Remove the resource with `id`. Returns `true` if a record
    /// was removed. Callers are responsible for checking dependents
    /// (secrets, pairs, permissions) before invoking this.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.resources.len();
        self.resources.retain(|r| r.id != id);
        self.resources.len() != before
    }
}

impl PairsDocument {
    /// Look up by primary key `(wrapper, resource_id)`.
    pub fn find(&self, wrapper: &str, resource_id: &str) -> Option<&WrapperResourcePairRecord> {
        self.pairs
            .iter()
            .find(|p| p.wrapper == wrapper && p.resource_id == resource_id)
    }

    /// Iterate over every pair that references a given resource id.
    /// Useful for cascade-refusal checks on resource delete.
    pub fn pairs_for_resource<'a>(
        &'a self,
        resource_id: &'a str,
    ) -> impl Iterator<Item = &'a WrapperResourcePairRecord> + 'a {
        self.pairs
            .iter()
            .filter(move |p| p.resource_id == resource_id)
    }

    /// Insert, refusing duplicates on `(wrapper, resource_id)`.
    pub fn insert(&mut self, record: WrapperResourcePairRecord) -> Result<(), InsertError> {
        if self.find(&record.wrapper, &record.resource_id).is_some() {
            return Err(InsertError::Duplicate);
        }
        self.pairs.push(record);
        Ok(())
    }

    /// Remove a pair. Returns `true` when a record was removed.
    pub fn remove(&mut self, wrapper: &str, resource_id: &str) -> bool {
        let before = self.pairs.len();
        self.pairs
            .retain(|p| !(p.wrapper == wrapper && p.resource_id == resource_id));
        self.pairs.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(id: &str, username: &str) -> EntityCreator {
        EntityCreator::Operator {
            id: id.to_string(),
            username: username.to_string(),
        }
    }

    fn ci(id: &str) -> EntityCreator {
        EntityCreator::ClientInstance { id: id.to_string() }
    }

    #[test]
    fn default_documents_are_empty_and_versioned() {
        let r = ResourcesDocument::default();
        assert_eq!(r.version, ENTITY_SCHEMA_VERSION);
        assert!(r.resources.is_empty());
        let p = PairsDocument::default();
        assert_eq!(p.version, ENTITY_SCHEMA_VERSION);
        assert!(p.pairs.is_empty());
    }

    #[test]
    fn resources_roundtrip_through_json_with_both_creator_kinds() {
        let mut doc = ResourcesDocument::default();
        doc.insert(ResourceRecord {
            id: "res_aaaa".into(),
            name: "github/token".into(),
            created_by: op("op-1", "alice"),
            created_at: 1_700_000_000,
        })
        .unwrap();
        doc.insert(ResourceRecord {
            id: "res_bbbb".into(),
            name: "gws/state".into(),
            created_by: ci("client-7"),
            created_at: 1_700_000_500,
        })
        .unwrap();

        let json = serde_json::to_string(&doc).unwrap();
        let parsed: ResourcesDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, doc);
    }

    #[test]
    fn pairs_roundtrip_through_json() {
        let mut doc = PairsDocument::default();
        doc.insert(WrapperResourcePairRecord {
            wrapper: "gh-comment".into(),
            resource_id: "res_aaaa".into(),
            created_by: ci("client-7"),
            created_at: 42,
            last_used_at: None,
        })
        .unwrap();
        let json = serde_json::to_string(&doc).unwrap();
        let parsed: PairsDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, doc);
    }

    #[test]
    fn unknown_version_is_rejected_by_callers() {
        // Round-trip leaves the version field as-is; callers must
        // check it on load. The default's value is well-defined.
        let raw = r#"{"version": 999, "resources": []}"#;
        let parsed: ResourcesDocument = serde_json::from_str(raw).unwrap();
        assert_ne!(parsed.version, ENTITY_SCHEMA_VERSION);
    }

    #[test]
    fn resource_insert_refuses_duplicate_name_or_id() {
        let mut doc = ResourcesDocument::default();
        doc.insert(ResourceRecord {
            id: "res_a".into(),
            name: "github/token".into(),
            created_by: op("op-1", "alice"),
            created_at: 1,
        })
        .unwrap();
        // Duplicate name, different id → rejected.
        let err = doc
            .insert(ResourceRecord {
                id: "res_b".into(),
                name: "github/token".into(),
                created_by: op("op-2", "bob"),
                created_at: 2,
            })
            .unwrap_err();
        assert_eq!(err, InsertError::Duplicate);
        // Duplicate id, different name → also rejected.
        let err = doc
            .insert(ResourceRecord {
                id: "res_a".into(),
                name: "github/other".into(),
                created_by: op("op-2", "bob"),
                created_at: 2,
            })
            .unwrap_err();
        assert_eq!(err, InsertError::Duplicate);
        // Provenance untouched.
        assert_eq!(doc.resources.len(), 1);
        match &doc.resources[0].created_by {
            EntityCreator::Operator { username, .. } => assert_eq!(username, "alice"),
            _ => panic!("creator changed"),
        }
        assert_eq!(doc.resources[0].created_at, 1);
    }

    #[test]
    fn resource_rename_preserves_id_and_refuses_collisions() {
        let mut doc = ResourcesDocument::default();
        doc.insert(ResourceRecord {
            id: "res_a".into(),
            name: "github/token".into(),
            created_by: op("op-1", "alice"),
            created_at: 1,
        })
        .unwrap();
        doc.insert(ResourceRecord {
            id: "res_b".into(),
            name: "gws/state".into(),
            created_by: op("op-1", "alice"),
            created_at: 2,
        })
        .unwrap();

        // Rename to a free name succeeds; the id is unchanged.
        assert!(doc.rename("res_a", "github/main").unwrap());
        assert_eq!(doc.find_by_id("res_a").unwrap().name, "github/main");

        // Renaming an unknown id is Ok(false), not an error.
        assert!(!doc.rename("res_zzz", "anything").unwrap());

        // Renaming onto another resource's name fails.
        let err = doc.rename("res_a", "gws/state").unwrap_err();
        assert_eq!(err, InsertError::Duplicate);

        // Renaming to the same name is a no-op — allowed.
        assert!(doc.rename("res_a", "github/main").unwrap());
    }

    #[test]
    fn pair_insert_refuses_duplicate_wrapper_resource_pair() {
        let mut doc = PairsDocument::default();
        doc.insert(WrapperResourcePairRecord {
            wrapper: "gh".into(),
            resource_id: "res_r".into(),
            created_by: ci("c1"),
            created_at: 1,
            last_used_at: None,
        })
        .unwrap();
        let err = doc
            .insert(WrapperResourcePairRecord {
                wrapper: "gh".into(),
                resource_id: "res_r".into(),
                created_by: ci("c2"),
                created_at: 2,
                last_used_at: None,
            })
            .unwrap_err();
        assert_eq!(err, InsertError::Duplicate);
        // Same wrapper, different resource — allowed.
        doc.insert(WrapperResourcePairRecord {
            wrapper: "gh".into(),
            resource_id: "res_r2".into(),
            created_by: ci("c1"),
            created_at: 3,
            last_used_at: None,
        })
        .unwrap();
        assert_eq!(doc.pairs.len(), 2);
    }

    #[test]
    fn pairs_for_resource_filters_correctly() {
        let mut doc = PairsDocument::default();
        doc.insert(WrapperResourcePairRecord {
            wrapper: "gh".into(),
            resource_id: "res_r".into(),
            created_by: ci("c1"),
            created_at: 1,
            last_used_at: None,
        })
        .unwrap();
        doc.insert(WrapperResourcePairRecord {
            wrapper: "tf".into(),
            resource_id: "res_r".into(),
            created_by: ci("c1"),
            created_at: 2,
            last_used_at: None,
        })
        .unwrap();
        doc.insert(WrapperResourcePairRecord {
            wrapper: "tf".into(),
            resource_id: "res_other".into(),
            created_by: ci("c1"),
            created_at: 3,
            last_used_at: None,
        })
        .unwrap();
        let mut wrappers: Vec<_> = doc
            .pairs_for_resource("res_r")
            .map(|p| p.wrapper.clone())
            .collect();
        wrappers.sort();
        assert_eq!(wrappers, vec!["gh".to_string(), "tf".to_string()]);
    }

    #[test]
    fn remove_returns_whether_anything_was_removed() {
        let mut r = ResourcesDocument::default();
        r.insert(ResourceRecord {
            id: "res_a".into(),
            name: "r1".into(),
            created_by: op("op", "a"),
            created_at: 1,
        })
        .unwrap();
        assert!(r.remove("res_a"));
        assert!(!r.remove("res_a"));

        let mut p = PairsDocument::default();
        p.insert(WrapperResourcePairRecord {
            wrapper: "gh".into(),
            resource_id: "res_r".into(),
            created_by: ci("c1"),
            created_at: 1,
            last_used_at: None,
        })
        .unwrap();
        assert!(p.remove("gh", "res_r"));
        assert!(!p.remove("gh", "res_r"));
    }
}
