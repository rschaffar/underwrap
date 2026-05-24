//! Structured libsecret admin surface under
//! `/v1/admin/stores/{sid}/resources/{rid}/libsecret/*`.
//!
//! Mirrors the Secret Service spec hierarchy directly: a Service-level
//! alias table plus a set of Collections, each holding Items. Every
//! mutating handler runs through `with_blob`, which performs one atomic
//! load → mutate → save cycle against the structured `LibsecretSection`
//! in the resource blob and saves only when the closure reports the
//! blob actually changed. None of these endpoints go through the legacy
//! flat-KV bridge.
//!
//! `created` is set on POST; `modified` is bumped server-side on every
//! successful mutation. Clients never supply either field. Item secret
//! bytes are omitted from the tree view and served on a dedicated
//! reveal endpoint so a tree fetch never carries secret material.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::{Collection, Item, ResourceBlob};
use underwrap_protocol::service::{
    AdminLibsecretAliasDeleteResponse, AdminLibsecretAliasUpsertRequest,
    AdminLibsecretCollectionCreateRequest, AdminLibsecretCollectionCreateResponse,
    AdminLibsecretCollectionDeleteResponse, AdminLibsecretCollectionPatchRequest,
    AdminLibsecretCollectionView, AdminLibsecretItemCreateRequest,
    AdminLibsecretItemCreateResponse, AdminLibsecretItemDeleteResponse,
    AdminLibsecretItemPatchRequest, AdminLibsecretItemSecretResponse, AdminLibsecretItemView,
    AdminLibsecretTreeResponse, AdminOkResponse, ServiceErrorResponse,
};
use underwrap_store::{load_resource_blob, save_resource_blob};

use crate::admin_api::AdminError;
use crate::admin_api::audit::{AuditEntity, LibsecretEntityKind, log_admin_event};
use crate::admin_api::body_b64::{decode_b64, encode_b64};
use crate::admin_api::stores::admin_store;
use crate::http::responses::Body;
use crate::runtime_state::AdminRuntimeState;
use crate::users::{Principal, now_epoch_secs};

const MAX_ID_LEN: usize = 128;

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_get_tree))
        .routes(routes!(handle_post_collection))
        .routes(routes!(handle_patch_collection, handle_delete_collection))
        .routes(routes!(handle_post_item))
        .routes(routes!(handle_patch_item, handle_delete_item))
        .routes(routes!(handle_get_item_secret))
        .routes(routes!(handle_put_alias, handle_delete_alias))
}

// ---- shared helpers -------------------------------------------------------

/// Validate a caller-supplied id (collection id, item id, alias name).
/// The rules — `[A-Za-z0-9._-]+`, max 128 chars, no leading `_` — keep
/// the id usable as a D-Bus path segment and reserve `_`-prefixed names
/// for future internal use.
fn validate_id(kind: &str, id: &str) -> Result<(), AdminError> {
    if id.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_request",
            format!("{kind} id must not be empty"),
        ));
    }
    if id.len() > MAX_ID_LEN {
        return Err(AdminError::bad_request(
            "invalid_request",
            format!("{kind} id exceeds {MAX_ID_LEN} characters"),
        ));
    }
    if id.starts_with('_') {
        return Err(AdminError::bad_request(
            "invalid_request",
            format!("{kind} id must not start with '_'"),
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(AdminError::bad_request(
            "invalid_request",
            format!("{kind} id contains disallowed characters; allowed: [A-Za-z0-9._-]"),
        ));
    }
    Ok(())
}

/// Generate the next free id of the shape `<prefix><n>` where `n` is
/// the lowest positive integer producing a key not already present in
/// `existing`.
fn next_auto_id<T>(prefix: &str, existing: &BTreeMap<String, T>) -> String {
    let mut n = 1u64;
    loop {
        let candidate = format!("{prefix}{n}");
        if !existing.contains_key(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Load the resource blob for a read-only handler.
fn read_blob(
    state: &AdminRuntimeState,
    store_id: &str,
    resource_id: &str,
) -> Result<ResourceBlob, AdminError> {
    let store = admin_store(state, store_id)?;
    load_resource_blob(store.as_ref(), resource_id)
        .map_err(|e| AdminError::internal("loading resource blob", e))
}

/// Load the blob, hand it to `f`, save it back if `f` reports
/// `dirty = true`. Concurrent writers race on the blob as a whole
/// (last writer wins), matching the rest of the admin surface.
fn with_blob<R>(
    state: &AdminRuntimeState,
    store_id: &str,
    resource_id: &str,
    f: impl FnOnce(&mut ResourceBlob) -> Result<(R, bool), AdminError>,
) -> Result<R, AdminError> {
    let store = admin_store(state, store_id)?;
    let mut blob = load_resource_blob(store.as_ref(), resource_id)
        .map_err(|e| AdminError::internal("loading resource blob", e))?;
    let (result, dirty) = f(&mut blob)?;
    if dirty {
        save_resource_blob(store.as_ref(), resource_id, &blob)
            .map_err(|e| AdminError::internal("saving resource blob", e))?;
    }
    Ok(result)
}

fn audit_result<T>(r: &Result<T, AdminError>) -> &'static str {
    if r.is_ok() { "success" } else { "failure" }
}

// ---- GET tree -------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/v1/admin/stores/{sid}/resources/{rid}/libsecret",
    tag = "libsecret",
    security(("session_cookie" = [])),
    params(
        ("sid" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
    ),
    responses(
        (status = 200, description = "Whole libsecret tree (item secrets omitted).", body = AdminLibsecretTreeResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_get_tree(
    State(state): State<Arc<AdminRuntimeState>>,
    Path((store_id, resource_id)): Path<(String, String)>,
) -> Result<Json<AdminLibsecretTreeResponse>, AdminError> {
    let section = read_blob(&state, &store_id, &resource_id)?.libsecret;
    let collections = section
        .collections
        .into_iter()
        .map(|(cid, col)| {
            let items = col
                .items
                .into_iter()
                .map(|(iid, item)| AdminLibsecretItemView {
                    id: iid,
                    label: item.label,
                    content_type: item.content_type,
                    created: item.created,
                    modified: item.modified,
                    attributes: item.attributes,
                })
                .collect();
            AdminLibsecretCollectionView {
                id: cid,
                label: col.label,
                created: col.created,
                modified: col.modified,
                items,
            }
        })
        .collect();
    Ok(Json(AdminLibsecretTreeResponse {
        aliases: section.aliases,
        collections,
    }))
}

// ---- Collection CRUD ------------------------------------------------------

#[utoipa::path(
    post,
    path = "/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections",
    tag = "libsecret",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("sid" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
    ),
    request_body = AdminLibsecretCollectionCreateRequest,
    responses(
        (status = 200, description = "Collection created.", body = AdminLibsecretCollectionCreateResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 409, description = "Collection id already exists.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_post_collection(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id)): Path<(String, String)>,
    Body(body): Body<AdminLibsecretCollectionCreateRequest>,
) -> Result<Json<AdminLibsecretCollectionCreateResponse>, AdminError> {
    let outcome = with_blob(&state, &store_id, &resource_id, |blob| {
        let id = match body.id {
            Some(requested) => {
                validate_id("collection", &requested)?;
                if blob.libsecret.collections.contains_key(&requested) {
                    return Err(AdminError::conflict(
                        "collection_exists",
                        format!("collection '{requested}' already exists"),
                    ));
                }
                requested
            }
            None => next_auto_id("c", &blob.libsecret.collections),
        };
        let now = now_epoch_secs();
        blob.libsecret.collections.insert(
            id.clone(),
            Collection {
                label: body.label,
                created: now,
                modified: now,
                items: BTreeMap::new(),
            },
        );
        Ok((AdminLibsecretCollectionCreateResponse { id }, true))
    });
    let entity_key = outcome
        .as_ref()
        .ok()
        .map(|r| r.id.clone())
        .unwrap_or_else(|| "<unknown>".to_string());
    log_admin_event(
        "libsecret.collection.create",
        &principal,
        AuditEntity::LibsecretEntity {
            store: &store_id,
            resource_id: &resource_id,
            kind: LibsecretEntityKind::Collection,
            path: &entity_key,
        },
        None,
        audit_result(&outcome),
    );
    outcome.map(Json)
}

#[utoipa::path(
    patch,
    path = "/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}",
    tag = "libsecret",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("sid" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("cid" = String, Path, description = "Collection id."),
    ),
    request_body = AdminLibsecretCollectionPatchRequest,
    responses(
        (status = 200, description = "Collection updated.", body = AdminOkResponse),
        (status = 404, description = "Collection not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_patch_collection(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id, cid)): Path<(String, String, String)>,
    Body(body): Body<AdminLibsecretCollectionPatchRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let outcome = with_blob(&state, &store_id, &resource_id, |blob| {
        let collection = blob
            .libsecret
            .collections
            .get_mut(&cid)
            .ok_or_else(|| AdminError::not_found(format!("collection '{cid}'")))?;
        let mut changed = false;
        if let Some(label) = body.label
            && label != collection.label
        {
            collection.label = label;
            changed = true;
        }
        if changed {
            collection.modified = now_epoch_secs();
        }
        Ok((AdminOkResponse { ok: true }, changed))
    });
    log_admin_event(
        "libsecret.collection.update",
        &principal,
        AuditEntity::LibsecretEntity {
            store: &store_id,
            resource_id: &resource_id,
            kind: LibsecretEntityKind::Collection,
            path: &cid,
        },
        None,
        audit_result(&outcome),
    );
    outcome.map(Json)
}

#[utoipa::path(
    delete,
    path = "/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}",
    tag = "libsecret",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("sid" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("cid" = String, Path, description = "Collection id."),
    ),
    responses(
        (status = 200, description = "Collection deleted; any aliases pointing at it are scrubbed.", body = AdminLibsecretCollectionDeleteResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_delete_collection(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id, cid)): Path<(String, String, String)>,
) -> Result<Json<AdminLibsecretCollectionDeleteResponse>, AdminError> {
    let outcome = with_blob(&state, &store_id, &resource_id, |blob| {
        let existed = blob.libsecret.collections.remove(&cid).is_some();
        // Scrub any aliases pointing at this id — including dangling
        // ones, where the alias target id was never an extant
        // collection. The operator gets a consistent post-condition
        // either way: after this call, no alias references `cid`.
        let aliases_scrubbed: Vec<String> = blob
            .libsecret
            .aliases
            .iter()
            .filter(|(_, target)| target.as_str() == cid)
            .map(|(name, _)| name.clone())
            .collect();
        for name in &aliases_scrubbed {
            blob.libsecret.aliases.remove(name);
        }
        let dirty = existed || !aliases_scrubbed.is_empty();
        Ok((
            AdminLibsecretCollectionDeleteResponse {
                existed,
                aliases_scrubbed,
            },
            dirty,
        ))
    });
    log_admin_event(
        "libsecret.collection.delete",
        &principal,
        AuditEntity::LibsecretEntity {
            store: &store_id,
            resource_id: &resource_id,
            kind: LibsecretEntityKind::Collection,
            path: &cid,
        },
        None,
        audit_result(&outcome),
    );
    outcome.map(Json)
}

// ---- Item CRUD + secret reveal -------------------------------------------

#[utoipa::path(
    post,
    path = "/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items",
    tag = "libsecret",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("sid" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("cid" = String, Path, description = "Collection id."),
    ),
    request_body = AdminLibsecretItemCreateRequest,
    responses(
        (status = 200, description = "Item created.", body = AdminLibsecretItemCreateResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Collection not found.", body = ServiceErrorResponse),
        (status = 409, description = "Item id already exists.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_post_item(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id, cid)): Path<(String, String, String)>,
    Body(body): Body<AdminLibsecretItemCreateRequest>,
) -> Result<Json<AdminLibsecretItemCreateResponse>, AdminError> {
    let outcome = (|| -> Result<AdminLibsecretItemCreateResponse, AdminError> {
        let secret = decode_b64(&body.secret_b64).map(SecretBytes::from)?;
        with_blob(&state, &store_id, &resource_id, |blob| {
            let collection = blob
                .libsecret
                .collections
                .get_mut(&cid)
                .ok_or_else(|| AdminError::not_found(format!("collection '{cid}'")))?;
            let id = match body.id {
                Some(requested) => {
                    validate_id("item", &requested)?;
                    if collection.items.contains_key(&requested) {
                        return Err(AdminError::conflict(
                            "item_exists",
                            format!("item '{requested}' already exists in collection '{cid}'"),
                        ));
                    }
                    requested
                }
                None => next_auto_id("item_", &collection.items),
            };
            let now = now_epoch_secs();
            collection.items.insert(
                id.clone(),
                Item {
                    label: body.label,
                    content_type: body.content_type,
                    created: now,
                    modified: now,
                    attributes: body.attributes,
                    secret,
                },
            );
            collection.modified = now;
            Ok((AdminLibsecretItemCreateResponse { id }, true))
        })
    })();
    let entity_key = match &outcome {
        Ok(resp) => format!("{cid}/{}", resp.id),
        Err(_) => format!("{cid}/<unknown>"),
    };
    log_admin_event(
        "libsecret.item.create",
        &principal,
        AuditEntity::LibsecretEntity {
            store: &store_id,
            resource_id: &resource_id,
            kind: LibsecretEntityKind::Item,
            path: &entity_key,
        },
        None,
        audit_result(&outcome),
    );
    outcome.map(Json)
}

#[utoipa::path(
    patch,
    path = "/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items/{iid}",
    tag = "libsecret",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("sid" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("cid" = String, Path, description = "Collection id."),
        ("iid" = String, Path, description = "Item id."),
    ),
    request_body = AdminLibsecretItemPatchRequest,
    responses(
        (status = 200, description = "Item updated.", body = AdminOkResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Collection or item not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_patch_item(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id, cid, iid)): Path<(String, String, String, String)>,
    Body(body): Body<AdminLibsecretItemPatchRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let touched_secret = body.secret_b64.is_some();
    let outcome = (|| -> Result<AdminOkResponse, AdminError> {
        let new_secret = match &body.secret_b64 {
            Some(b64) => Some(decode_b64(b64).map(SecretBytes::from)?),
            None => None,
        };
        with_blob(&state, &store_id, &resource_id, |blob| {
            let collection = blob
                .libsecret
                .collections
                .get_mut(&cid)
                .ok_or_else(|| AdminError::not_found(format!("collection '{cid}'")))?;
            let item = collection.items.get_mut(&iid).ok_or_else(|| {
                AdminError::not_found(format!("item '{iid}' in collection '{cid}'"))
            })?;
            let mut changed = false;
            if let Some(label) = body.label
                && label != item.label
            {
                item.label = label;
                changed = true;
            }
            if let Some(ct) = body.content_type
                && ct != item.content_type
            {
                item.content_type = ct;
                changed = true;
            }
            if let Some(attrs) = body.attributes
                && attrs != item.attributes
            {
                item.attributes = attrs;
                changed = true;
            }
            if let Some(secret) = new_secret
                && secret != item.secret
            {
                item.secret = secret;
                changed = true;
            }
            if changed {
                let now = now_epoch_secs();
                item.modified = now;
                collection.modified = now;
            }
            Ok((AdminOkResponse { ok: true }, changed))
        })
    })();
    let entity_key = format!("{cid}/{iid}");
    let result = audit_result(&outcome);
    log_admin_event(
        "libsecret.item.update",
        &principal,
        AuditEntity::LibsecretEntity {
            store: &store_id,
            resource_id: &resource_id,
            kind: LibsecretEntityKind::Item,
            path: &entity_key,
        },
        None,
        result,
    );
    if touched_secret {
        log_admin_event(
            "libsecret.item.set_secret",
            &principal,
            AuditEntity::LibsecretEntity {
                store: &store_id,
                resource_id: &resource_id,
                kind: LibsecretEntityKind::Item,
                path: &entity_key,
            },
            None,
            result,
        );
    }
    outcome.map(Json)
}

#[utoipa::path(
    delete,
    path = "/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items/{iid}",
    tag = "libsecret",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("sid" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("cid" = String, Path, description = "Collection id."),
        ("iid" = String, Path, description = "Item id."),
    ),
    responses(
        (status = 200, description = "Item removed (existed flag in body).", body = AdminLibsecretItemDeleteResponse),
        (status = 404, description = "Collection not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_delete_item(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id, cid, iid)): Path<(String, String, String, String)>,
) -> Result<Json<AdminLibsecretItemDeleteResponse>, AdminError> {
    let outcome = with_blob(&state, &store_id, &resource_id, |blob| {
        let collection = blob
            .libsecret
            .collections
            .get_mut(&cid)
            .ok_or_else(|| AdminError::not_found(format!("collection '{cid}'")))?;
        let existed = collection.items.remove(&iid).is_some();
        if existed {
            collection.modified = now_epoch_secs();
        }
        Ok((AdminLibsecretItemDeleteResponse { existed }, existed))
    });
    let entity_key = format!("{cid}/{iid}");
    log_admin_event(
        "libsecret.item.delete",
        &principal,
        AuditEntity::LibsecretEntity {
            store: &store_id,
            resource_id: &resource_id,
            kind: LibsecretEntityKind::Item,
            path: &entity_key,
        },
        None,
        audit_result(&outcome),
    );
    outcome.map(Json)
}

#[utoipa::path(
    get,
    path = "/v1/admin/stores/{sid}/resources/{rid}/libsecret/collections/{cid}/items/{iid}/secret",
    tag = "libsecret",
    security(("session_cookie" = [])),
    params(
        ("sid" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("cid" = String, Path, description = "Collection id."),
        ("iid" = String, Path, description = "Item id."),
    ),
    responses(
        (status = 200, description = "Item secret material, base64-encoded.", body = AdminLibsecretItemSecretResponse),
        (status = 404, description = "Collection or item not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_get_item_secret(
    State(state): State<Arc<AdminRuntimeState>>,
    Path((store_id, resource_id, cid, iid)): Path<(String, String, String, String)>,
) -> Result<Json<AdminLibsecretItemSecretResponse>, AdminError> {
    let blob = read_blob(&state, &store_id, &resource_id)?;
    let collection = blob
        .libsecret
        .collections
        .get(&cid)
        .ok_or_else(|| AdminError::not_found(format!("collection '{cid}'")))?;
    let item = collection
        .items
        .get(&iid)
        .ok_or_else(|| AdminError::not_found(format!("item '{iid}' in collection '{cid}'")))?;
    Ok(Json(AdminLibsecretItemSecretResponse {
        secret_b64: encode_b64(item.secret.as_ref()),
    }))
}

// ---- Alias CRUD -----------------------------------------------------------

#[utoipa::path(
    put,
    path = "/v1/admin/stores/{sid}/resources/{rid}/libsecret/aliases/{name}",
    tag = "libsecret",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("sid" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("name" = String, Path, description = "Alias name."),
    ),
    request_body = AdminLibsecretAliasUpsertRequest,
    responses(
        (status = 200, description = "Alias set.", body = AdminOkResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Collection not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_put_alias(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id, name)): Path<(String, String, String)>,
    Body(body): Body<AdminLibsecretAliasUpsertRequest>,
) -> Result<Json<AdminOkResponse>, AdminError> {
    let outcome = (|| -> Result<AdminOkResponse, AdminError> {
        validate_id("alias", &name)?;
        with_blob(&state, &store_id, &resource_id, |blob| {
            let target = body.collection_id;
            if !blob.libsecret.collections.contains_key(&target) {
                return Err(AdminError::not_found(format!("collection '{target}'")));
            }
            let dirty = blob.libsecret.aliases.get(&name) != Some(&target);
            if dirty {
                blob.libsecret.aliases.insert(name.clone(), target);
            }
            Ok((AdminOkResponse { ok: true }, dirty))
        })
    })();
    log_admin_event(
        "libsecret.alias.set",
        &principal,
        AuditEntity::LibsecretEntity {
            store: &store_id,
            resource_id: &resource_id,
            kind: LibsecretEntityKind::Alias,
            path: &name,
        },
        None,
        audit_result(&outcome),
    );
    outcome.map(Json)
}

#[utoipa::path(
    delete,
    path = "/v1/admin/stores/{sid}/resources/{rid}/libsecret/aliases/{name}",
    tag = "libsecret",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(
        ("sid" = String, Path, description = "Store id."),
        ("rid" = String, Path, description = "Resource id."),
        ("name" = String, Path, description = "Alias name."),
    ),
    responses(
        (status = 200, description = "Alias removed (existed flag in body).", body = AdminLibsecretAliasDeleteResponse),
        (status = 404, description = "Store not found.", body = ServiceErrorResponse),
        (status = 423, description = "Store is locked.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_delete_alias(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path((store_id, resource_id, name)): Path<(String, String, String)>,
) -> Result<Json<AdminLibsecretAliasDeleteResponse>, AdminError> {
    let outcome = with_blob(&state, &store_id, &resource_id, |blob| {
        let existed = blob.libsecret.aliases.remove(&name).is_some();
        Ok((AdminLibsecretAliasDeleteResponse { existed }, existed))
    });
    log_admin_event(
        "libsecret.alias.delete",
        &principal,
        AuditEntity::LibsecretEntity {
            store: &store_id,
            resource_id: &resource_id,
            kind: LibsecretEntityKind::Alias,
            path: &name,
        },
        None,
        audit_result(&outcome),
    );
    outcome.map(Json)
}

#[cfg(test)]
mod tests;
