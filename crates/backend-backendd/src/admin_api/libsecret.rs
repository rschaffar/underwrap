//! Structured libsecret admin surface under
//! `/v1/admin/stores/{sid}/resources/{rid}/libsecret/*`.
//!
//! Mirrors the Secret Service spec hierarchy directly: a Service-level
//! alias table plus a set of Collections, each holding Items. Every
//! handler performs one atomic `load_resource_blob` → mutate →
//! `save_resource_blob` cycle against the structured `LibsecretSection`
//! in the resource blob. None of these endpoints go through the legacy
//! flat-KV bridge (`expand_blob_to_entries`); the synthetic `_aliases`
//! / `<col>/_meta` / `<item>.meta` / `<item>.secret` keys never enter
//! the admin surface.
//!
//! `created` is set on POST; `modified` is bumped server-side on every
//! successful mutation. Clients never supply either field. Item secret
//! bytes are omitted from the tree view and served on a dedicated
//! reveal endpoint so a tree fetch never carries secret material.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use base64::Engine;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use underwrap_protocol::message::SecretBytes;
use underwrap_protocol::resource_blob::{Collection, Item, LibsecretSection};
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
    for n in 1u64.. {
        let candidate = format!("{prefix}{n}");
        if !existing.contains_key(&candidate) {
            return candidate;
        }
    }
    unreachable!("u64 exhausted while picking a libsecret id")
}

fn load_section(
    state: &AdminRuntimeState,
    store_id: &str,
    resource_id: &str,
) -> Result<(Arc<dyn underwrap_store::StorageBackend>, LibsecretSection), AdminError> {
    let store = admin_store(state, store_id)?;
    let blob = load_resource_blob(store.as_ref(), resource_id)
        .map_err(|e| AdminError::internal("loading resource blob", e))?;
    Ok((store, blob.libsecret))
}

fn save_section(
    store: &dyn underwrap_store::StorageBackend,
    resource_id: &str,
    section: LibsecretSection,
) -> Result<(), AdminError> {
    let mut blob = load_resource_blob(store, resource_id)
        .map_err(|e| AdminError::internal("reloading resource blob", e))?;
    blob.libsecret = section;
    save_resource_blob(store, resource_id, &blob)
        .map_err(|e| AdminError::internal("saving resource blob", e))
}

fn decode_secret_b64(b64: &str) -> Result<SecretBytes, AdminError> {
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map(SecretBytes::from)
        .map_err(|e| {
            AdminError::bad_request("invalid_request", format!("invalid base64 secret: {e}"))
        })
}

fn audit_libsecret<'a>(
    store: &'a str,
    resource_id: &'a str,
    kind: LibsecretEntityKind,
    path: &'a str,
) -> AuditEntity<'a> {
    AuditEntity::LibsecretEntity {
        store,
        resource_id,
        kind,
        path,
    }
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
    let (_store, section) = load_section(&state, &store_id, &resource_id)?;
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
    let outcome = post_collection_inner(&state, &store_id, &resource_id, body).await;
    let (entity_key, result) = match &outcome {
        Ok(resp) => (resp.id.clone(), "success"),
        Err(_) => ("<unknown>".to_string(), "failure"),
    };
    log_admin_event(
        "libsecret.collection.create",
        &principal,
        audit_libsecret(
            &store_id,
            &resource_id,
            LibsecretEntityKind::Collection,
            &entity_key,
        ),
        None,
        result,
    );
    outcome.map(Json)
}

async fn post_collection_inner(
    state: &AdminRuntimeState,
    store_id: &str,
    resource_id: &str,
    body: AdminLibsecretCollectionCreateRequest,
) -> Result<AdminLibsecretCollectionCreateResponse, AdminError> {
    let (store, mut section) = load_section(state, store_id, resource_id)?;

    let id = match body.id {
        Some(requested) => {
            validate_id("collection", &requested)?;
            if section.collections.contains_key(&requested) {
                return Err(AdminError::conflict(
                    "collection_exists",
                    format!("collection '{requested}' already exists"),
                ));
            }
            requested
        }
        None => next_auto_id("c", &section.collections),
    };

    let now = now_epoch_secs();
    section.collections.insert(
        id.clone(),
        Collection {
            label: body.label,
            created: now,
            modified: now,
            items: BTreeMap::new(),
        },
    );
    save_section(store.as_ref(), resource_id, section)?;
    Ok(AdminLibsecretCollectionCreateResponse { id })
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
    let outcome = patch_collection_inner(&state, &store_id, &resource_id, &cid, body).await;
    log_admin_event(
        "libsecret.collection.update",
        &principal,
        audit_libsecret(
            &store_id,
            &resource_id,
            LibsecretEntityKind::Collection,
            &cid,
        ),
        None,
        if outcome.is_ok() {
            "success"
        } else {
            "failure"
        },
    );
    outcome.map(Json)
}

async fn patch_collection_inner(
    state: &AdminRuntimeState,
    store_id: &str,
    resource_id: &str,
    cid: &str,
    body: AdminLibsecretCollectionPatchRequest,
) -> Result<AdminOkResponse, AdminError> {
    let (store, mut section) = load_section(state, store_id, resource_id)?;
    let collection = section
        .collections
        .get_mut(cid)
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
        save_section(store.as_ref(), resource_id, section)?;
    }
    Ok(AdminOkResponse { ok: true })
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
    let outcome = delete_collection_inner(&state, &store_id, &resource_id, &cid).await;
    log_admin_event(
        "libsecret.collection.delete",
        &principal,
        audit_libsecret(
            &store_id,
            &resource_id,
            LibsecretEntityKind::Collection,
            &cid,
        ),
        None,
        if outcome.is_ok() {
            "success"
        } else {
            "failure"
        },
    );
    outcome.map(Json)
}

async fn delete_collection_inner(
    state: &AdminRuntimeState,
    store_id: &str,
    resource_id: &str,
    cid: &str,
) -> Result<AdminLibsecretCollectionDeleteResponse, AdminError> {
    let (store, mut section) = load_section(state, store_id, resource_id)?;
    let existed = section.collections.remove(cid).is_some();
    let aliases_scrubbed: Vec<String> = section
        .aliases
        .iter()
        .filter(|(_, target)| target.as_str() == cid)
        .map(|(name, _)| name.clone())
        .collect();
    for name in &aliases_scrubbed {
        section.aliases.remove(name);
    }
    if existed || !aliases_scrubbed.is_empty() {
        save_section(store.as_ref(), resource_id, section)?;
    }
    Ok(AdminLibsecretCollectionDeleteResponse {
        existed,
        aliases_scrubbed,
    })
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
    let outcome = post_item_inner(&state, &store_id, &resource_id, &cid, body).await;
    let (entity_key, result) = match &outcome {
        Ok(resp) => (format!("{cid}/{}", resp.id), "success"),
        Err(_) => (format!("{cid}/<unknown>"), "failure"),
    };
    log_admin_event(
        "libsecret.item.create",
        &principal,
        audit_libsecret(
            &store_id,
            &resource_id,
            LibsecretEntityKind::Item,
            &entity_key,
        ),
        None,
        result,
    );
    outcome.map(Json)
}

async fn post_item_inner(
    state: &AdminRuntimeState,
    store_id: &str,
    resource_id: &str,
    cid: &str,
    body: AdminLibsecretItemCreateRequest,
) -> Result<AdminLibsecretItemCreateResponse, AdminError> {
    let secret = decode_secret_b64(&body.secret_b64)?;
    let (store, mut section) = load_section(state, store_id, resource_id)?;
    let collection = section
        .collections
        .get_mut(cid)
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
    save_section(store.as_ref(), resource_id, section)?;
    Ok(AdminLibsecretItemCreateResponse { id })
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
    let outcome = patch_item_inner(&state, &store_id, &resource_id, &cid, &iid, body).await;
    let result_str = if outcome.is_ok() {
        "success"
    } else {
        "failure"
    };
    let entity_key = format!("{cid}/{iid}");
    log_admin_event(
        "libsecret.item.update",
        &principal,
        audit_libsecret(
            &store_id,
            &resource_id,
            LibsecretEntityKind::Item,
            &entity_key,
        ),
        None,
        result_str,
    );
    if touched_secret {
        log_admin_event(
            "libsecret.item.set_secret",
            &principal,
            audit_libsecret(
                &store_id,
                &resource_id,
                LibsecretEntityKind::Item,
                &entity_key,
            ),
            None,
            result_str,
        );
    }
    outcome.map(Json)
}

async fn patch_item_inner(
    state: &AdminRuntimeState,
    store_id: &str,
    resource_id: &str,
    cid: &str,
    iid: &str,
    body: AdminLibsecretItemPatchRequest,
) -> Result<AdminOkResponse, AdminError> {
    let new_secret = match &body.secret_b64 {
        Some(b64) => Some(decode_secret_b64(b64)?),
        None => None,
    };
    let (store, mut section) = load_section(state, store_id, resource_id)?;
    let collection = section
        .collections
        .get_mut(cid)
        .ok_or_else(|| AdminError::not_found(format!("collection '{cid}'")))?;
    let item = collection
        .items
        .get_mut(iid)
        .ok_or_else(|| AdminError::not_found(format!("item '{iid}' in collection '{cid}'")))?;

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
        save_section(store.as_ref(), resource_id, section)?;
    }
    Ok(AdminOkResponse { ok: true })
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
    let outcome = delete_item_inner(&state, &store_id, &resource_id, &cid, &iid).await;
    let entity_key = format!("{cid}/{iid}");
    log_admin_event(
        "libsecret.item.delete",
        &principal,
        audit_libsecret(
            &store_id,
            &resource_id,
            LibsecretEntityKind::Item,
            &entity_key,
        ),
        None,
        if outcome.is_ok() {
            "success"
        } else {
            "failure"
        },
    );
    outcome.map(Json)
}

async fn delete_item_inner(
    state: &AdminRuntimeState,
    store_id: &str,
    resource_id: &str,
    cid: &str,
    iid: &str,
) -> Result<AdminLibsecretItemDeleteResponse, AdminError> {
    let (store, mut section) = load_section(state, store_id, resource_id)?;
    let collection = section
        .collections
        .get_mut(cid)
        .ok_or_else(|| AdminError::not_found(format!("collection '{cid}'")))?;
    let existed = collection.items.remove(iid).is_some();
    if existed {
        collection.modified = now_epoch_secs();
        save_section(store.as_ref(), resource_id, section)?;
    }
    Ok(AdminLibsecretItemDeleteResponse { existed })
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
    let (_store, section) = load_section(&state, &store_id, &resource_id)?;
    let collection = section
        .collections
        .get(&cid)
        .ok_or_else(|| AdminError::not_found(format!("collection '{cid}'")))?;
    let item = collection
        .items
        .get(&iid)
        .ok_or_else(|| AdminError::not_found(format!("item '{iid}' in collection '{cid}'")))?;
    let bytes: &[u8] = item.secret.as_ref();
    Ok(Json(AdminLibsecretItemSecretResponse {
        secret_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
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
    let outcome = put_alias_inner(&state, &store_id, &resource_id, &name, body).await;
    log_admin_event(
        "libsecret.alias.set",
        &principal,
        audit_libsecret(&store_id, &resource_id, LibsecretEntityKind::Alias, &name),
        None,
        if outcome.is_ok() {
            "success"
        } else {
            "failure"
        },
    );
    outcome.map(Json)
}

async fn put_alias_inner(
    state: &AdminRuntimeState,
    store_id: &str,
    resource_id: &str,
    name: &str,
    body: AdminLibsecretAliasUpsertRequest,
) -> Result<AdminOkResponse, AdminError> {
    validate_id("alias", name)?;
    let (store, mut section) = load_section(state, store_id, resource_id)?;
    if !section.collections.contains_key(&body.collection_id) {
        return Err(AdminError::not_found(format!(
            "collection '{}'",
            body.collection_id
        )));
    }
    let prev = section.aliases.insert(name.to_string(), body.collection_id);
    let changed = prev.as_ref() != section.aliases.get(name);
    if changed {
        save_section(store.as_ref(), resource_id, section)?;
    }
    Ok(AdminOkResponse { ok: true })
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
    let outcome = delete_alias_inner(&state, &store_id, &resource_id, &name).await;
    log_admin_event(
        "libsecret.alias.delete",
        &principal,
        audit_libsecret(&store_id, &resource_id, LibsecretEntityKind::Alias, &name),
        None,
        if outcome.is_ok() {
            "success"
        } else {
            "failure"
        },
    );
    outcome.map(Json)
}

async fn delete_alias_inner(
    state: &AdminRuntimeState,
    store_id: &str,
    resource_id: &str,
    name: &str,
) -> Result<AdminLibsecretAliasDeleteResponse, AdminError> {
    let (store, mut section) = load_section(state, store_id, resource_id)?;
    let existed = section.aliases.remove(name).is_some();
    if existed {
        save_section(store.as_ref(), resource_id, section)?;
    }
    Ok(AdminLibsecretAliasDeleteResponse { existed })
}

#[cfg(test)]
mod tests;
