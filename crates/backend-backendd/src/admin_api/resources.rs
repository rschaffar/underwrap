//! Resource admin CRUD: `/v1/admin/resources/{list, create, delete,
//! rename, blob/get, blob/put}`.
//!
//! Resources are first-class entities with provenance, stored in the
//! resources document and projected to one canonical blob per
//! resource. Delete refuses with structured blockers if secrets,
//! pairs, or wrapper permissions still reference the resource.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::routing::post;
use base64::Engine;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

use underwrap_store::{load_resource_blob, save_resource_blob};

use crate::admin_api::audit::log_admin_event;
use crate::admin_api::stores::admin_store;
use crate::http::responses::{error_response, json_response, parse_body};
use crate::runtime_api::wrappers::generate_resource_id;
use crate::runtime_state::AdminRuntimeState;
use crate::storage::documents::{
    ENTITY_DEPENDENT_SAMPLE_LIMIT, load_pairs_document, load_policy_document,
    load_resources_document, resource_secret_dependents, save_resources_document,
};
use crate::users::Principal;

pub(crate) fn write_admin_resource_audit(
    operator: &str,
    resource_id: &str,
    action: &str,
    operation: Option<&str>,
    result: &str,
) {
    let resource = format!("resource:{resource_id}");
    log_admin_event(
        action,
        operator,
        None,
        Some(&resource),
        None,
        operation,
        result,
    );
}

pub(crate) fn handle_admin_resources_list(
    state: &AdminRuntimeState,
    body: &underwrap_protocol::service::AdminResourcesListRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    match load_resources_document(store.as_ref()) {
        Ok(mut doc) => {
            doc.resources.sort_by(|a, b| a.name.cmp(&b.name));
            json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminResourcesListResponse {
                    resources: doc.resources,
                },
            )
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("loading resources document: {e:#}"),
        ),
    }
}

pub(crate) fn handle_admin_resource_blob_get(
    state: &AdminRuntimeState,
    body: &underwrap_protocol::service::AdminResourceBlobGetRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let blob = match load_resource_blob(store.as_ref(), &body.id) {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading resource blob: {e:#}"),
            );
        }
    };
    let bytes = match blob.to_canonical_bytes() {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("canonicalizing resource blob: {e:#}"),
            );
        }
    };
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminResourceBlobGetResponse {
            blob_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
        },
    )
}

pub(crate) fn handle_admin_resource_blob_put(
    state: &AdminRuntimeState,
    operator: &str,
    body: &underwrap_protocol::service::AdminResourceBlobPutRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::resource_blob::ResourceBlob;
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let blob_bytes = match base64::engine::general_purpose::STANDARD.decode(&body.blob_b64) {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("blob_b64 is not valid base64: {e}"),
            );
        }
    };
    let blob = match ResourceBlob::from_canonical_bytes(&blob_bytes) {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_blob",
                &format!("decoding resource blob: {e:#}"),
            );
        }
    };
    // Enforce that the caller sent already-canonical bytes. The
    // wrapper-side does this naturally via serde_jcs; admin clients
    // must as well.
    let canonical = match blob.to_canonical_bytes() {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("canonicalizing resource blob: {e:#}"),
            );
        }
    };
    if canonical != blob_bytes {
        return error_response(
            StatusCode::BAD_REQUEST,
            "noncanonical_blob",
            "blob_b64 is not JCS-canonical; re-encode via serde_jcs",
        );
    }
    if let Err(e) = save_resource_blob(store.as_ref(), &body.id, &blob) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("saving resource blob: {e:#}"),
        );
    }
    write_admin_resource_audit(operator, &body.id, "resource.blob.put", None, "success");
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminResourceBlobPutResponse { ok: true },
    )
}

pub(crate) fn handle_admin_resource_create(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: &underwrap_protocol::service::AdminResourceCreateRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::entities::{EntityCreator, ResourceRecord};
    let name = body.name.trim();
    if name.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_resource_name",
            "resource name must be non-empty",
        );
    }
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_resources_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading resources document: {e:#}"),
            );
        }
    };
    if doc.find_by_name(name).is_some() {
        return error_response(
            StatusCode::CONFLICT,
            "resource_already_exists",
            &format!("resource '{name}' already exists"),
        );
    }
    let record = ResourceRecord {
        id: generate_resource_id(),
        name: name.to_string(),
        created_by: EntityCreator::Operator {
            id: principal.id.clone(),
            username: principal.username.clone(),
        },
        created_at: crate::users::now_epoch_secs(),
    };
    doc.insert(record.clone())
        .expect("find returned None just above");
    if let Err(e) = save_resources_document(store.as_ref(), &doc) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting resources document: {e:#}"),
        );
    }
    write_admin_resource_audit(
        &principal.username,
        &record.id,
        "resource.create",
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminResourceCreateResponse { resource: record },
    )
}

pub(crate) fn handle_admin_resource_delete(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: &underwrap_protocol::service::AdminResourceDeleteRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::service::{
        AdminResourceDeleteBlockers, AdminResourceDeleteResponse, AdminWrapperPermissionKey,
        ServiceWrapperResourcePair,
    };
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_resources_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading resources document: {e:#}"),
            );
        }
    };
    let Some(target) = doc.find_by_id(&body.id) else {
        return error_response(StatusCode::NOT_FOUND, "not_found", "unknown resource");
    };
    let target_name = target.name.clone();

    let (secret_count, sample_secret_ids) =
        match resource_secret_dependents(store.as_ref(), &body.id) {
            Ok(v) => v,
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    &format!("scanning resource dependents: {e:#}"),
                );
            }
        };

    let pairs = match load_pairs_document(store.as_ref()) {
        Ok(p) => p,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading pairs document: {e:#}"),
            );
        }
    };
    // The wire shape for blocker pairs still carries the *name* in
    // `resource` so the operator gets a readable label; resolve via the
    // resources document.
    let dependent_pairs: Vec<ServiceWrapperResourcePair> = pairs
        .pairs_for_resource(&body.id)
        .map(|p| ServiceWrapperResourcePair {
            wrapper: p.wrapper.clone(),
            resource: target_name.clone(),
        })
        .collect();
    let pair_count = u32::try_from(dependent_pairs.len()).unwrap_or(u32::MAX);
    let mut sample_pairs = dependent_pairs;
    sample_pairs.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);

    let policy = match load_policy_document(store.as_ref()) {
        Ok(p) => p,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading policy document: {e:#}"),
            );
        }
    };
    let dependent_permissions: Vec<AdminWrapperPermissionKey> = policy
        .permissions
        .iter()
        .filter(|p| p.resource_id == body.id)
        .map(|p| AdminWrapperPermissionKey {
            client_instance: p.client_instance.clone(),
            wrapper: p.wrapper.clone(),
            resource_id: p.resource_id.clone(),
        })
        .collect();
    let permission_count = u32::try_from(dependent_permissions.len()).unwrap_or(u32::MAX);
    let mut sample_permissions = dependent_permissions;
    sample_permissions.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);

    if secret_count > 0 || pair_count > 0 || permission_count > 0 {
        write_admin_resource_audit(
            &principal.username,
            &body.id,
            "resource.delete",
            None,
            "blocked",
        );
        return json_response(
            StatusCode::OK,
            &AdminResourceDeleteResponse {
                ok: false,
                blocked_by: Some(AdminResourceDeleteBlockers {
                    secret_count,
                    sample_secret_ids,
                    pair_count,
                    sample_pairs,
                    permission_count,
                    sample_permissions,
                }),
            },
        );
    }

    doc.remove(&body.id);
    if let Err(e) = save_resources_document(store.as_ref(), &doc) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting resources document: {e:#}"),
        );
    }
    write_admin_resource_audit(
        &principal.username,
        &body.id,
        "resource.delete",
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &AdminResourceDeleteResponse {
            ok: true,
            blocked_by: None,
        },
    )
}

pub(crate) fn handle_admin_resource_rename(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: &underwrap_protocol::service::AdminResourceRenameRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::entities::InsertError;
    let new_name = body.new_name.trim();
    if new_name.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_resource_name",
            "new_name must be non-empty",
        );
    }
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_resources_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading resources document: {e:#}"),
            );
        }
    };
    match doc.rename(&body.id, new_name) {
        Ok(true) => {}
        Ok(false) => {
            return error_response(StatusCode::NOT_FOUND, "not_found", "unknown resource");
        }
        Err(InsertError::Duplicate) => {
            return error_response(
                StatusCode::CONFLICT,
                "resource_already_exists",
                &format!("resource name '{new_name}' is already taken"),
            );
        }
    }
    let updated = doc
        .find_by_id(&body.id)
        .expect("rename above returned Ok(true)")
        .clone();
    if let Err(e) = save_resources_document(store.as_ref(), &doc) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting resources document: {e:#}"),
        );
    }
    write_admin_resource_audit(
        &principal.username,
        &body.id,
        "resource.rename",
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminResourceRenameResponse { resource: updated },
    )
}

pub(crate) fn router() -> Router<Arc<AdminRuntimeState>> {
    Router::new()
        .route("/v1/admin/resources/list", post(axum_list))
        .route("/v1/admin/resources/create", post(axum_create))
        .route("/v1/admin/resources/delete", post(axum_delete))
        .route("/v1/admin/resources/rename", post(axum_rename))
        .route("/v1/admin/resources/blob/get", post(axum_blob_get))
        .route("/v1/admin/resources/blob/put", post(axum_blob_put))
}

async fn axum_list(
    State(state): State<Arc<AdminRuntimeState>>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminResourcesListRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_resources_list(&state, &parsed)
}

async fn axum_create(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminResourceCreateRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_resource_create(&state, &principal, &parsed)
}

async fn axum_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminResourceDeleteRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_resource_delete(&state, &principal, &parsed)
}

async fn axum_rename(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminResourceRenameRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_resource_rename(&state, &principal, &parsed)
}

async fn axum_blob_get(
    State(state): State<Arc<AdminRuntimeState>>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminResourceBlobGetRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_resource_blob_get(&state, &parsed)
}

async fn axum_blob_put(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminResourceBlobPutRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_resource_blob_put(&state, &principal.username, &parsed)
}

#[cfg(test)]
mod tests;
