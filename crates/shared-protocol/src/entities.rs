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
//! document keyed at a reserved `__underwrap/...` secret id.
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

/// Reserved secret id used to persist the [`ResourcesDocument`].
pub const RESOURCES_DOCUMENT_SECRET_ID: &str = "__underwrap/resources/document";
/// Reserved secret id used to persist the [`PairsDocument`].
pub const PAIRS_DOCUMENT_SECRET_ID: &str = "__underwrap/pairs/document";

/// Who created an entity record. Distinguishes admin operators (who
/// register resources/pairs by hand through the admin API) from
/// runtime client instances (which auto-create on startup when their
/// `auto_create_wrapper_assets` flag is on).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct ResourcesDocument {
    #[serde(default)]
    pub resources: Vec<ResourceRecord>,
}

/// Store-backed pairs document.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct PairsDocument {
    #[serde(default)]
    pub pairs: Vec<WrapperResourcePairRecord>,
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
mod tests;
