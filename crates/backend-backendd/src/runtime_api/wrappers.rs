//! `POST /v1/wrappers` — register the daemon's `(wrapper_name,
//! resource_name)` pairs at startup. Diffs the request against the
//! resource + pair documents, auto-creates the missing entities if the
//! calling client instance is opted in, or refuses with a structured
//! list of what would have been created.

use std::collections::{BTreeSet, HashMap};

use axum::Json;

use underwrap_protocol::entities::{
    EntityCreator, PairsDocument, ResourceRecord, ResourcesDocument, WrapperResourcePairRecord,
};
use underwrap_protocol::policy::{PolicyDocument, WrapperApprovalMode, WrapperPermission};
use underwrap_protocol::service::{
    RuntimeWrapperRegisterPair, RuntimeWrapperRegisterRequest, RuntimeWrapperRegisterResponse,
};
use underwrap_store::StorageBackend;

use crate::runtime_api::error::{IntoInternal, RuntimeError};
use crate::runtime_state::ListenerState;
use crate::storage::documents::{
    load_pairs_document, load_policy_document, load_resources_document, save_pairs_document,
    save_policy_document, save_resources_document,
};

/// Pre-write diff between an incoming register request and the
/// resource/pair documents on disk. `resolved_resource_ids` maps each
/// incoming `resource_name` whose record already exists to its id;
/// `missing_resources` is the unresolved set; `missing_pairs` is every
/// incoming row whose `(wrapper, resource_id)` is not yet paired (or
/// whose resource isn't yet resolved at all).
#[derive(Debug, Default)]
struct RegisterDiff {
    resolved_resource_ids: HashMap<String, String>,
    missing_resources: BTreeSet<String>,
    missing_pairs: Vec<RuntimeWrapperRegisterPair>,
}

impl RegisterDiff {
    fn is_empty(&self) -> bool {
        self.missing_resources.is_empty() && self.missing_pairs.is_empty()
    }
}

/// Reject empty wrapper/resource names up front; everything downstream
/// assumes those are non-empty.
fn validate_register_request(body: &RuntimeWrapperRegisterRequest) -> Result<(), RuntimeError> {
    for pair in &body.wrappers {
        if pair.wrapper_name.trim().is_empty() {
            return Err(RuntimeError::bad_request(
                "invalid_wrapper_name",
                "wrapper_name must be non-empty",
            ));
        }
        if pair.resource_name.trim().is_empty() {
            return Err(RuntimeError::bad_request(
                "invalid_resource_name",
                "resource_name must be non-empty",
            ));
        }
    }
    Ok(())
}

/// Compute what's missing from the resource/pair documents relative to
/// the incoming request. Pure function — no IO, no mutation; the
/// driver decides whether to commit or refuse based on the result.
fn compute_register_diff(
    body: &RuntimeWrapperRegisterRequest,
    resources: &ResourcesDocument,
    pairs: &PairsDocument,
) -> RegisterDiff {
    let mut diff = RegisterDiff::default();
    for pair in &body.wrappers {
        match resources.find_by_name(&pair.resource_name) {
            Some(record) => {
                diff.resolved_resource_ids
                    .insert(pair.resource_name.clone(), record.id.clone());
            }
            None => {
                diff.missing_resources.insert(pair.resource_name.clone());
            }
        }
    }
    diff.missing_pairs = body
        .wrappers
        .iter()
        .filter(|p| match diff.resolved_resource_ids.get(&p.resource_name) {
            Some(rid) => pairs.find(&p.wrapper_name, rid).is_none(),
            None => true,
        })
        .cloned()
        .collect();
    diff
}

/// Materialise the missing resources and pairs into the documents.
/// Updates the diff's `resolved_resource_ids` map in place so the
/// caller can use it for permission seeding.
fn apply_auto_create(
    resources: &mut ResourcesDocument,
    pairs: &mut PairsDocument,
    diff: &mut RegisterDiff,
    creator: &EntityCreator,
    now: u64,
) -> (Vec<String>, Vec<RuntimeWrapperRegisterPair>) {
    let mut created_resources: Vec<String> = Vec::new();
    for name in &diff.missing_resources {
        let id = crate::ids::random_id("res");
        resources
            .insert(ResourceRecord {
                id: id.clone(),
                name: name.clone(),
                created_by: creator.clone(),
                created_at: now,
            })
            .expect("missing-set guarantees no duplicate");
        diff.resolved_resource_ids.insert(name.clone(), id);
        created_resources.push(name.clone());
    }
    let mut created_pairs: Vec<RuntimeWrapperRegisterPair> = Vec::new();
    for pair in &diff.missing_pairs {
        let rid = diff
            .resolved_resource_ids
            .get(&pair.resource_name)
            .expect("resource id resolved for every missing pair after auto-create");
        pairs
            .insert(WrapperResourcePairRecord {
                wrapper: pair.wrapper_name.clone(),
                resource_id: rid.clone(),
                created_by: creator.clone(),
                created_at: now,
                last_used_at: None,
            })
            .expect("missing-set guarantees no duplicate");
        created_pairs.push(pair.clone());
    }
    (created_resources, created_pairs)
}

/// Seed a `Locked` permission for every incoming
/// `(client_instance, wrapper, resource_id)` whose resource id is
/// resolved and that doesn't already have a permission row. Returns
/// `true` when the policy document was modified — callers persist only
/// in that case.
fn seed_locked_permissions(
    policy: &mut PolicyDocument,
    body: &RuntimeWrapperRegisterRequest,
    resolved: &HashMap<String, String>,
    principal: &str,
) -> bool {
    let mut dirty = false;
    for incoming in &body.wrappers {
        let Some(rid) = resolved.get(&incoming.resource_name) else {
            continue;
        };
        let already = policy.permissions.iter().any(|p| {
            p.client_instance == principal
                && p.wrapper == incoming.wrapper_name
                && p.resource_id == *rid
        });
        if !already {
            policy.permissions.push(WrapperPermission {
                client_instance: principal.to_string(),
                wrapper: incoming.wrapper_name.clone(),
                resource_id: rid.clone(),
                mode: WrapperApprovalMode::Locked,
                writable: false,
                reason: None,
            });
            dirty = true;
        }
    }
    dirty
}

/// Handle `POST /v1/wrappers`. Loads the three control-plane
/// documents, dispatches between no-op / refusal / commit, and
/// persists exactly what changed.
pub(crate) fn handle_runtime_wrapper_register(
    state: &ListenerState,
    store: &dyn StorageBackend,
    principal: &str,
    body: &RuntimeWrapperRegisterRequest,
) -> Result<Json<RuntimeWrapperRegisterResponse>, RuntimeError> {
    validate_register_request(body)?;

    let auto_create_enabled = state
        .shared
        .client_instances
        .read()
        .unwrap()
        .get(principal)
        .map(|r| r.auto_create_wrapper_assets)
        .unwrap_or(false);
    let now = crate::time::now_epoch_secs();

    let mut resources =
        load_resources_document(store).into_internal("loading resources document")?;
    let mut pairs = load_pairs_document(store).into_internal("loading pairs document")?;
    let mut diff = compute_register_diff(body, &resources, &pairs);

    if diff.is_empty() {
        // No-op fast path. May still need to seed `Locked` permissions
        // for a fresh `client_instance` re-using pairs another instance
        // already created.
        let mut policy = load_policy_document(store).into_internal("loading policy document")?;
        if seed_locked_permissions(&mut policy, body, &diff.resolved_resource_ids, principal) {
            save_policy_document(store, &policy).into_internal("saving policy document")?;
        }
        return Ok(Json(RuntimeWrapperRegisterResponse::Ok {
            auto_created_resources: Vec::new(),
            auto_created_pairs: Vec::new(),
        }));
    }

    if !auto_create_enabled {
        return Ok(Json(RuntimeWrapperRegisterResponse::AutoCreateDisabled {
            missing_resources: diff.missing_resources.into_iter().collect(),
            missing_pairs: diff.missing_pairs,
        }));
    }

    let creator = EntityCreator::ClientInstance {
        id: principal.to_string(),
    };
    let (auto_created_resources, auto_created_pairs) =
        apply_auto_create(&mut resources, &mut pairs, &mut diff, &creator, now);

    let mut policy = load_policy_document(store).into_internal("loading policy document")?;
    let policy_dirty =
        seed_locked_permissions(&mut policy, body, &diff.resolved_resource_ids, principal);

    if !auto_created_resources.is_empty() {
        save_resources_document(store, &resources).into_internal("saving resources document")?;
    }
    if !auto_created_pairs.is_empty() {
        save_pairs_document(store, &pairs).into_internal("saving pairs document")?;
    }
    if policy_dirty {
        save_policy_document(store, &policy).into_internal("saving policy document")?;
    }

    Ok(Json(RuntimeWrapperRegisterResponse::Ok {
        auto_created_resources,
        auto_created_pairs,
    }))
}

#[cfg(test)]
mod tests;
