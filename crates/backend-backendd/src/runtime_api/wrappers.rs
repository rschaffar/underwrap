//! `POST /v1/wrappers` — register the daemon's `(wrapper_name,
//! resource_name)` pairs at startup. Diffs the request against the
//! resource + pair documents, auto-creates the missing entities if the
//! calling client instance is opted in, or refuses with a structured
//! list of what would have been created.

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

use underwrap_protocol::policy::PolicyDocument;
use underwrap_store::StorageBackend;

use crate::http::responses::{error_response, json_response};
use crate::runtime_state::AdminRuntimeState;
use crate::storage::documents::{
    load_pairs_document, load_policy_document_or_default, load_resources_document,
    save_pairs_document, save_policy_document, save_resources_document,
};

/// Handle `POST /v1/wrappers`.
///
/// Computes the diff between the request's pairs and what's already
/// in the resource/pair documents, then either auto-creates the
/// missing entities (if the calling client instance is opted in) or
/// returns a structured refusal listing what would have been created.
/// On the happy path the documents are written back; on the refusal
/// path the store is untouched. Permissions for newly-paired
/// `(client_instance, wrapper, resource)` rows are seeded `Locked`
/// so a freshly-registered daemon shows up as deny-by-default in the
/// admin SPA.
pub(crate) fn handle_runtime_wrapper_register(
    state: &AdminRuntimeState,
    store: &dyn StorageBackend,
    principal: &str,
    body: &underwrap_protocol::service::RuntimeWrapperRegisterRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::entities::{EntityCreator, ResourceRecord, WrapperResourcePairRecord};
    use underwrap_protocol::policy::{WrapperApprovalMode, WrapperPermission};
    use underwrap_protocol::service::{RuntimeWrapperRegisterPair, RuntimeWrapperRegisterResponse};

    for pair in &body.wrappers {
        if pair.wrapper_name.trim().is_empty() {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_wrapper_name",
                "wrapper_name must be non-empty",
            );
        }
        if pair.resource_name.trim().is_empty() {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_resource_name",
                "resource_name must be non-empty",
            );
        }
    }

    let auto_create_enabled = state
        .client_instances
        .read()
        .unwrap()
        .get(principal)
        .map(|r| r.auto_create_wrapper_assets)
        .unwrap_or(false);
    let now = crate::users::now_epoch_secs();

    let mut resources = match load_resources_document(store) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading resources document: {e:#}"),
            );
        }
    };
    let mut pairs = match load_pairs_document(store) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading pairs document: {e:#}"),
            );
        }
    };

    // Build the (incoming resource name) → (resolved resource id) map.
    let mut resolved_resource_ids: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut missing_resources_set: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    for pair in &body.wrappers {
        match resources.find_by_name(&pair.resource_name) {
            Some(record) => {
                resolved_resource_ids.insert(pair.resource_name.clone(), record.id.clone());
            }
            None => {
                missing_resources_set.insert(pair.resource_name.clone());
            }
        }
    }

    // A "missing pair" is any incoming pair whose (wrapper, resolved
    // resource id) is not already in the pairs document. Pairs whose
    // resource isn't even resolved yet (auto-create disabled and a new
    // resource name was sent) trivially count as missing.
    let missing_pairs: Vec<RuntimeWrapperRegisterPair> = body
        .wrappers
        .iter()
        .filter(|p| match resolved_resource_ids.get(&p.resource_name) {
            Some(rid) => pairs.find(&p.wrapper_name, rid).is_none(),
            None => true,
        })
        .cloned()
        .collect();

    // Auto-seed a `Locked` permission for every incoming
    // `(client_instance, wrapper, resource_id)` whose resource id is
    // resolved. Computed now, persisted only on committing branches.
    let seed_locked_permissions = |policy: &mut PolicyDocument,
                                   resolved: &std::collections::HashMap<String, String>|
     -> bool {
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
    };

    if missing_resources_set.is_empty() && missing_pairs.is_empty() {
        // No-op fast path. Still may need to seed Locked permissions
        // for a new `client_instance` re-using pairs another instance
        // already created.
        let mut policy = match load_policy_document_or_default(store) {
            Ok(p) => p,
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &format!("loading policy: {e:#}"),
                );
            }
        };
        if seed_locked_permissions(&mut policy, &resolved_resource_ids)
            && let Err(e) = save_policy_document(store, &policy)
        {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("saving policy: {e:#}"),
            );
        }
        return json_response(
            StatusCode::OK,
            &RuntimeWrapperRegisterResponse::Ok {
                auto_created_resources: Vec::new(),
                auto_created_pairs: Vec::new(),
            },
        );
    }

    if !auto_create_enabled {
        return json_response(
            StatusCode::OK,
            &RuntimeWrapperRegisterResponse::AutoCreateDisabled {
                missing_resources: missing_resources_set.into_iter().collect(),
                missing_pairs,
            },
        );
    }

    let creator = EntityCreator::ClientInstance {
        id: principal.to_string(),
    };

    let mut auto_created_resources: Vec<String> = Vec::new();
    for name in &missing_resources_set {
        let id = generate_resource_id();
        resources
            .insert(ResourceRecord {
                id: id.clone(),
                name: name.clone(),
                created_by: creator.clone(),
                created_at: now,
            })
            .expect("missing-set guarantees no duplicate");
        resolved_resource_ids.insert(name.clone(), id);
        auto_created_resources.push(name.clone());
    }

    let mut auto_created_pairs: Vec<RuntimeWrapperRegisterPair> = Vec::new();
    for pair in &missing_pairs {
        let rid = resolved_resource_ids
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
        auto_created_pairs.push(pair.clone());
    }

    let mut policy = match load_policy_document_or_default(store) {
        Ok(p) => p,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading policy: {e:#}"),
            );
        }
    };
    let policy_dirty = seed_locked_permissions(&mut policy, &resolved_resource_ids);

    if !auto_created_resources.is_empty()
        && let Err(e) = save_resources_document(store, &resources)
    {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("saving resources: {e:#}"),
        );
    }
    if !auto_created_pairs.is_empty()
        && let Err(e) = save_pairs_document(store, &pairs)
    {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("saving pairs: {e:#}"),
        );
    }
    if policy_dirty && let Err(e) = save_policy_document(store, &policy) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("saving policy: {e:#}"),
        );
    }

    json_response(
        StatusCode::OK,
        &RuntimeWrapperRegisterResponse::Ok {
            auto_created_resources,
            auto_created_pairs,
        },
    )
}

/// Generate a stable resource id (`res_<10 hex bytes>`). Mirrors
/// `generate_client_instance_id` in shape so the two id namespaces
/// look related but distinct.
pub(crate) fn generate_resource_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 10];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!(
        "res_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}
