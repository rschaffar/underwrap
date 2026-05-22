//! Policy document + wrapper-permission admin CRUD.
//!
//! Two URL families share this module because they manipulate the same
//! backing document (`POLICY_DOCUMENT_SECRET_ID`):
//!
//! - `/v1/admin/policy/{get, put}` — read/write the whole policy
//!   document.
//! - `/v1/admin/wrapper-permissions/{list, upsert, delete}` — granular
//!   read-modify-write on the `permissions` vec.
//!
//! Plus `drop_client_instance_permissions`, which the client-instance
//! admin module calls when an instance is reassigned to a different
//! store.

use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use axum::extract::State;
use axum::routing::post;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

use crate::admin_api::audit::log_admin_event;
use crate::admin_api::stores::admin_store;
use crate::http::responses::{error_response, json_response, parse_body};
use crate::runtime_state::AdminRuntimeState;
use crate::storage::documents::{load_policy_document, save_policy_document};
use crate::users::Principal;

/// Strip every wrapper permission whose `client_instance` matches
/// `client_instance_id` from the policy document of `store_id`. Used
/// when a client instance is reassigned to a different store, so its
/// authorizations on the previous store do not silently resurface if
/// it is ever remapped back.
pub(crate) fn drop_client_instance_permissions(
    state: &AdminRuntimeState,
    store_id: &str,
    client_instance_id: &str,
) -> Result<()> {
    let store = match state.stores.read().unwrap().unlocked_backend(store_id) {
        Ok(s) => s,
        // Skip when locked or unknown — the unlock path will be the
        // place to apply this kind of cleanup if it ever matters.
        Err(_) => return Ok(()),
    };
    let mut doc = load_policy_document(store.as_ref())?;
    let before = doc.permissions.len();
    doc.permissions
        .retain(|p| p.client_instance != client_instance_id);
    if doc.permissions.len() == before {
        return Ok(());
    }
    save_policy_document(store.as_ref(), &doc)
}

pub(crate) fn write_admin_permission_audit(
    actor: &crate::users::Principal,
    store_name: &str,
    permission: &underwrap_protocol::policy::WrapperPermission,
    action: &str,
    operation: Option<&str>,
    result: &str,
) {
    let resource = format!(
        "wrapper_permission:{store_name}/{}/{}/{}",
        permission.client_instance, permission.wrapper, permission.resource_id
    );
    log_admin_event(
        action,
        &actor.username,
        Some(permission.wrapper.as_str()),
        Some(resource.as_str()),
        None,
        operation,
        result,
    );
}

pub(crate) fn handle_admin_policy_get(
    state: &AdminRuntimeState,
    body: &underwrap_protocol::service::AdminPolicyGetRequest,
) -> Response<Full<Bytes>> {
    match admin_store(state, &body.store) {
        Ok(store) => match load_policy_document(store.as_ref()) {
            Ok(doc) => json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminPolicyGetResponse { document: doc },
            ),
            Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
        },
        Err(resp) => resp,
    }
}

pub(crate) fn handle_admin_policy_put(
    state: &AdminRuntimeState,
    body: &underwrap_protocol::service::AdminPolicyPutRequest,
) -> Response<Full<Bytes>> {
    match admin_store(state, &body.store) {
        Ok(store) => match save_policy_document(store.as_ref(), &body.document) {
            Ok(()) => json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminOkResponse { ok: true },
            ),
            Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
        },
        Err(resp) => resp,
    }
}

pub(crate) fn handle_admin_wrapper_permissions_list(
    state: &AdminRuntimeState,
    body: &underwrap_protocol::service::AdminWrapperPermissionsListRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    match load_policy_document(store.as_ref()) {
        Ok(doc) => json_response(
            StatusCode::OK,
            &underwrap_protocol::service::AdminWrapperPermissionsListResponse {
                permissions: doc.permissions,
            },
        ),
        Err(e) => error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string()),
    }
}

pub(crate) fn handle_admin_wrapper_permission_upsert(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: &underwrap_protocol::service::AdminWrapperPermissionUpsertRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_policy_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
        }
    };
    if body.permission.client_instance.is_empty()
        || body.permission.wrapper.is_empty()
        || body.permission.resource_id.is_empty()
    {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_permission",
            "client_instance, wrapper, and resource_id must all be non-empty",
        );
    }
    if let Some(slot) = doc.permissions.iter_mut().find(|p| {
        p.client_instance == body.permission.client_instance
            && p.wrapper == body.permission.wrapper
            && p.resource_id == body.permission.resource_id
    }) {
        *slot = body.permission.clone();
    } else {
        doc.permissions.push(body.permission.clone());
    }
    doc.permissions.sort_by(|a, b| {
        a.client_instance
            .cmp(&b.client_instance)
            .then(a.wrapper.cmp(&b.wrapper))
            .then(a.resource_id.cmp(&b.resource_id))
    });
    if let Err(e) = save_policy_document(store.as_ref(), &doc) {
        write_admin_permission_audit(
            principal,
            &body.store,
            &body.permission,
            "wrapper_permission.upsert",
            None,
            "failure",
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting policy document: {e:#}"),
        );
    }
    write_admin_permission_audit(
        principal,
        &body.store,
        &body.permission,
        "wrapper_permission.upsert",
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminOkResponse { ok: true },
    )
}

pub(crate) fn handle_admin_wrapper_permission_delete(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: &underwrap_protocol::service::AdminWrapperPermissionDeleteRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_policy_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(StatusCode::BAD_REQUEST, "admin_error", &e.to_string());
        }
    };
    let before = doc.permissions.len();
    doc.permissions.retain(|p| {
        !(p.client_instance == body.client_instance
            && p.wrapper == body.wrapper
            && p.resource_id == body.resource_id)
    });
    let removed = before != doc.permissions.len();
    if !removed {
        return error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "no matching wrapper permission",
        );
    }
    if let Err(e) = save_policy_document(store.as_ref(), &doc) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting policy document: {e:#}"),
        );
    }
    let removed_permission = underwrap_protocol::policy::WrapperPermission {
        client_instance: body.client_instance.clone(),
        wrapper: body.wrapper.clone(),
        resource_id: body.resource_id.clone(),
        mode: underwrap_protocol::policy::WrapperApprovalMode::Locked,
        writable: false,
        reason: None,
    };
    write_admin_permission_audit(
        principal,
        &body.store,
        &removed_permission,
        "wrapper_permission.delete",
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminOkResponse { ok: true },
    )
}

pub(crate) fn router() -> Router<Arc<AdminRuntimeState>> {
    Router::new()
        .route("/v1/admin/policy/get", post(axum_policy_get))
        .route("/v1/admin/policy/put", post(axum_policy_put))
        .route(
            "/v1/admin/wrapper-permissions/list",
            post(axum_permissions_list),
        )
        .route(
            "/v1/admin/wrapper-permissions/upsert",
            post(axum_permission_upsert),
        )
        .route(
            "/v1/admin/wrapper-permissions/delete",
            post(axum_permission_delete),
        )
}

async fn axum_policy_get(
    State(state): State<Arc<AdminRuntimeState>>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminPolicyGetRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_policy_get(&state, &parsed)
}

async fn axum_policy_put(
    State(state): State<Arc<AdminRuntimeState>>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminPolicyPutRequest = match parse_body(&body) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    handle_admin_policy_put(&state, &parsed)
}

async fn axum_permissions_list(
    State(state): State<Arc<AdminRuntimeState>>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminWrapperPermissionsListRequest =
        match parse_body(&body) {
            Ok(p) => p,
            Err(resp) => return resp,
        };
    handle_admin_wrapper_permissions_list(&state, &parsed)
}

async fn axum_permission_upsert(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminWrapperPermissionUpsertRequest =
        match parse_body(&body) {
            Ok(p) => p,
            Err(resp) => return resp,
        };
    handle_admin_wrapper_permission_upsert(&state, &principal, &parsed)
}

async fn axum_permission_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    body: Bytes,
) -> Response<Full<Bytes>> {
    let parsed: underwrap_protocol::service::AdminWrapperPermissionDeleteRequest =
        match parse_body(&body) {
            Ok(p) => p,
            Err(resp) => return resp,
        };
    handle_admin_wrapper_permission_delete(&state, &principal, &parsed)
}
