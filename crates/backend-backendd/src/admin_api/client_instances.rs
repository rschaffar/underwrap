//! Client-instance admin CRUD: `/v1/admin/client-instances/{list,
//! detail, create, update, delete}`.
//!
//! Client instances are the mTLS identities under which frontend
//! daemons authenticate to the backend's runtime API. Each instance
//! maps to one store; a remap drops the prior-store's wrapper
//! permissions so authorizations don't silently survive across the
//! reassignment.

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use std::collections::HashMap;

use underwrap_protocol::service::{
    AdminClientInstanceActionState, AdminClientInstanceCreateRequest,
    AdminClientInstanceDetailResponse, AdminClientInstanceListItem, AdminClientInstanceStatus,
    AdminClientInstanceUpdateRequest, AdminClientInstancesListResponse,
};

use crate::admin_api::audit::log_admin_event;
use crate::admin_api::policy::drop_client_instance_permissions;
use crate::http::responses::{error_response, json_response, parse_json_body};
use crate::runtime_state::AdminRuntimeState;

fn client_instance_status_to_wire(
    status: crate::runtime_clients::ClientInstanceStatus,
) -> AdminClientInstanceStatus {
    match status {
        crate::runtime_clients::ClientInstanceStatus::Active => AdminClientInstanceStatus::Active,
        crate::runtime_clients::ClientInstanceStatus::Disabled => {
            AdminClientInstanceStatus::Disabled
        }
    }
}

fn client_instance_status_from_wire(
    status: AdminClientInstanceStatus,
) -> crate::runtime_clients::ClientInstanceStatus {
    match status {
        AdminClientInstanceStatus::Active => crate::runtime_clients::ClientInstanceStatus::Active,
        AdminClientInstanceStatus::Disabled => {
            crate::runtime_clients::ClientInstanceStatus::Disabled
        }
    }
}

fn admin_client_instance_actions(
    record: &crate::runtime_clients::ClientInstanceRecord,
) -> AdminClientInstanceActionState {
    AdminClientInstanceActionState {
        can_edit_display_name: true,
        can_remap_store: true,
        can_disable: record.status == crate::runtime_clients::ClientInstanceStatus::Active,
        can_enable: record.status == crate::runtime_clients::ClientInstanceStatus::Disabled,
        can_delete: true,
    }
}

fn current_store_names(state: &AdminRuntimeState) -> HashMap<String, String> {
    state
        .stores
        .read()
        .unwrap()
        .records
        .iter()
        .map(|(id, loaded)| (id.clone(), loaded.record.name.clone()))
        .collect()
}

fn admin_client_instance_list_item(
    record: &crate::runtime_clients::ClientInstanceRecord,
    current_store_name: String,
    actions: AdminClientInstanceActionState,
) -> AdminClientInstanceListItem {
    AdminClientInstanceListItem {
        id: record.id.clone(),
        display_name: record.display_name.clone(),
        current_store_id: record.current_store_id.clone(),
        current_store_name,
        status: client_instance_status_to_wire(record.status),
        last_seen_at: record.last_seen_at,
        current_credential_fingerprint: record.current_credential.cert_fingerprint.clone(),
        auto_create_wrapper_assets: record.auto_create_wrapper_assets,
        actions,
    }
}

fn admin_client_instance_detail(
    record: &crate::runtime_clients::ClientInstanceRecord,
    current_store_name: String,
    actions: AdminClientInstanceActionState,
) -> AdminClientInstanceDetailResponse {
    AdminClientInstanceDetailResponse {
        id: record.id.clone(),
        display_name: record.display_name.clone(),
        current_store_id: record.current_store_id.clone(),
        current_store_name,
        status: client_instance_status_to_wire(record.status),
        last_seen_at: record.last_seen_at,
        current_credential_fingerprint: record.current_credential.cert_fingerprint.clone(),
        current_credential_issued_at: record.current_credential.issued_at,
        auto_create_wrapper_assets: record.auto_create_wrapper_assets,
        actions,
    }
}

pub(crate) fn handle_admin_client_instances_list(
    state: &AdminRuntimeState,
) -> Response<Full<Bytes>> {
    let store_names = current_store_names(state);
    let mut items = Vec::new();
    for record in state.client_instances.read().unwrap().values() {
        let Some(current_store_name) = store_names.get(&record.current_store_id).cloned() else {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "unknown_store",
                "client instance references an unknown store",
            );
        };
        let actions = admin_client_instance_actions(record);
        items.push(admin_client_instance_list_item(
            record,
            current_store_name,
            actions,
        ));
    }
    items.sort_by(|a, b| a.display_name.cmp(&b.display_name).then(a.id.cmp(&b.id)));
    json_response(StatusCode::OK, &AdminClientInstancesListResponse { items })
}

pub(crate) fn handle_admin_client_instance_detail(
    state: &AdminRuntimeState,
    id: &str,
) -> Response<Full<Bytes>> {
    let store_names = current_store_names(state);
    match state.client_instances.read().unwrap().get(id) {
        Some(record) => {
            let Some(current_store_name) = store_names.get(&record.current_store_id).cloned()
            else {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "unknown_store",
                    "client instance references an unknown store",
                );
            };
            let actions = admin_client_instance_actions(record);
            json_response(
                StatusCode::OK,
                &admin_client_instance_detail(record, current_store_name, actions),
            )
        }
        None => error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown client instance",
        ),
    }
}

pub(crate) async fn handle_admin_client_instance_create_endpoint(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminClientInstanceCreateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    handle_admin_client_instance_create(state, principal, &body)
}

pub(crate) fn handle_admin_client_instance_create(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: &AdminClientInstanceCreateRequest,
) -> Response<Full<Bytes>> {
    let display_name = body.display_name.trim();
    if display_name.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_display_name",
            "display name must not be empty",
        );
    }
    let display_name = display_name.to_string();
    let current_credential = match crate::runtime_clients::current_credential_from_pem(
        body.cert_pem.clone(),
        body.issued_at,
    ) {
        Ok(current) => current,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_client_certificate",
                &format!("parsing client certificate: {e:#}"),
            );
        }
    };
    let cert_fingerprint = current_credential.cert_fingerprint.clone();
    {
        let stores = state.stores.read().unwrap();
        if !stores.records.contains_key(&body.current_store_id) {
            return error_response(StatusCode::BAD_REQUEST, "unknown_store", "unknown store");
        }
    }
    let id = crate::ids::fresh_id("inst", |candidate| {
        state
            .client_instances
            .read()
            .unwrap()
            .contains_key(candidate)
    });
    let mut instances = state.client_instances.write().unwrap();
    if instances
        .values()
        .any(|existing| existing.display_name.eq_ignore_ascii_case(&display_name))
    {
        return error_response(
            StatusCode::CONFLICT,
            "display_name_already_exists",
            &format!(
                "client instance display name '{}' already exists",
                display_name
            ),
        );
    }
    if instances
        .values()
        .any(|existing| existing.current_credential.cert_fingerprint == cert_fingerprint)
    {
        return error_response(
            StatusCode::CONFLICT,
            "credential_already_exists",
            "client credential fingerprint already exists",
        );
    }
    let record = crate::runtime_clients::ClientInstanceRecord {
        id: id.clone(),
        display_name,
        current_store_id: body.current_store_id.clone(),
        status: client_instance_status_from_wire(body.status),
        last_seen_at: None,
        current_credential,
        auto_create_wrapper_assets: body.auto_create_wrapper_assets.unwrap_or(false),
    };
    instances.insert(id.clone(), record.clone());
    if let Err(e) =
        crate::runtime_clients::save_client_instances(&state.client_instances_file, &instances)
    {
        instances.remove(&id);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "write_client_instances_failed",
            &format!("{e:#}"),
        );
    }
    log_admin_event(
        "client-instance.create",
        &principal.username,
        None,
        Some(format!("client-instance:{id}").as_str()),
        None,
        None,
        "success",
    );
    let current_store_name = {
        let stores = state.stores.read().unwrap();
        match stores.records.get(&record.current_store_id) {
            Some(store) => store.record.name.clone(),
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "unknown_store",
                    "client instance references an unknown store",
                );
            }
        }
    };
    let actions = admin_client_instance_actions(&record);
    json_response(
        StatusCode::OK,
        &admin_client_instance_detail(&record, current_store_name, actions),
    )
}

pub(crate) async fn handle_admin_client_instance_update_endpoint(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    id: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminClientInstanceUpdateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    handle_admin_client_instance_update(state, principal, id, &body)
}

pub(crate) fn handle_admin_client_instance_update(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    id: &str,
    body: &AdminClientInstanceUpdateRequest,
) -> Response<Full<Bytes>> {
    let mut instances = state.client_instances.write().unwrap();
    let Some(existing) = instances.get(id).cloned() else {
        return error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown client instance",
        );
    };
    let old_store_id = existing.current_store_id.clone();
    let mut updated = existing;
    if let Some(display_name) = &body.display_name {
        let display_name = display_name.trim();
        if display_name.is_empty() {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_display_name",
                "display name must not be empty",
            );
        }
        let display_name = display_name.to_string();
        if instances.iter().any(|(other_id, other)| {
            other_id != id && other.display_name.eq_ignore_ascii_case(&display_name)
        }) {
            return error_response(
                StatusCode::CONFLICT,
                "display_name_already_exists",
                &format!(
                    "client instance display name '{}' already exists",
                    display_name
                ),
            );
        }
        updated.display_name = display_name;
    }
    if let Some(current_store_id) = &body.current_store_id {
        if !state
            .stores
            .read()
            .unwrap()
            .records
            .contains_key(current_store_id)
        {
            return error_response(StatusCode::BAD_REQUEST, "unknown_store", "unknown store");
        }
        updated.current_store_id = current_store_id.clone();
    }
    if let Some(status) = body.status {
        updated.status = client_instance_status_from_wire(status);
    }
    if let Some(cert_pem) = &body.cert_pem {
        let now = crate::users::now_epoch_secs();
        let new_credential = match crate::runtime_clients::current_credential_from_pem(
            cert_pem.clone(),
            Some(now),
        ) {
            Ok(current) => current,
            Err(e) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_client_certificate",
                    &format!("parsing client certificate: {e:#}"),
                );
            }
        };
        if instances.iter().any(|(other_id, other)| {
            other_id != id
                && other.current_credential.cert_fingerprint == new_credential.cert_fingerprint
        }) {
            return error_response(
                StatusCode::CONFLICT,
                "credential_already_exists",
                "client credential fingerprint already exists",
            );
        }
        updated.current_credential = new_credential;
    }
    if let Some(flag) = body.auto_create_wrapper_assets {
        updated.auto_create_wrapper_assets = flag;
    }
    instances.insert(id.to_string(), updated.clone());
    if let Err(e) =
        crate::runtime_clients::save_client_instances(&state.client_instances_file, &instances)
    {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "write_client_instances_failed",
            &format!("{e:#}"),
        );
    }
    drop(instances);
    if old_store_id != updated.current_store_id
        && let Err(e) = drop_client_instance_permissions(state, &old_store_id, id)
    {
        // Best-effort: log and continue. The remap already persisted
        // to disk, so failing the response with 500 would leave the
        // caller thinking the update did not take while the registry
        // already holds the new mapping. Mirrors the delete path's
        // treatment of the same cascade.
        tracing::warn!(
            client_instance = %id,
            store = %old_store_id,
            err = %format!("{e:#}"),
            "failed to strip wrapper permissions for remapped client instance",
        );
    }
    log_admin_event(
        "client-instance.update",
        &principal.username,
        None,
        Some(format!("client-instance:{id}").as_str()),
        None,
        None,
        "success",
    );
    let current_store_name = {
        let stores = state.stores.read().unwrap();
        match stores.records.get(&updated.current_store_id) {
            Some(store) => store.record.name.clone(),
            None => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "unknown_store",
                    "client instance references an unknown store",
                );
            }
        }
    };
    let actions = admin_client_instance_actions(&updated);
    json_response(
        StatusCode::OK,
        &admin_client_instance_detail(&updated, current_store_name, actions),
    )
}

pub(crate) fn handle_admin_client_instance_delete(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    id: &str,
) -> Response<Full<Bytes>> {
    let mut instances = state.client_instances.write().unwrap();
    let Some(removed) = instances.remove(id) else {
        return error_response(
            StatusCode::NOT_FOUND,
            "not_found",
            "unknown client instance",
        );
    };
    if let Err(e) =
        crate::runtime_clients::save_client_instances(&state.client_instances_file, &instances)
    {
        // Roll the in-memory state back so the next request sees the same
        // view as the on-disk file.
        instances.insert(id.to_string(), removed);
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "write_client_instances_failed",
            &format!("{e:#}"),
        );
    }
    drop(instances);
    // Cascade: strip every wrapper permission keyed on this client
    // instance id from the policy document of the store the instance
    // was last bound to. Without this, the rows linger forever as
    // "stale" entries that the SPA renders as nameless columns in the
    // wrapper-permissions table and that grant the same authorization
    // back if the same client instance id is ever reused.
    if let Err(e) = drop_client_instance_permissions(state, &removed.current_store_id, id) {
        // Best-effort: log and continue. The instance is already gone from
        // the registry, so leaving orphaned policy rows is preferable to
        // failing the delete (and leaving the operator with no way to
        // remove the client instance at all).
        tracing::warn!(
            client_instance = %id,
            store = %removed.current_store_id,
            err = %format!("{e:#}"),
            "failed to strip wrapper permissions for deleted client instance",
        );
    }
    log_admin_event(
        "client-instance.delete",
        &principal.username,
        None,
        Some(format!("client-instance:{id}").as_str()),
        None,
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminOkResponse { ok: true },
    )
}

pub(crate) async fn route(
    method: &Method,
    path: &str,
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    if path == "/v1/admin/client-instances" {
        return match *method {
            Method::GET => handle_admin_client_instances_list(state),
            Method::POST => {
                handle_admin_client_instance_create_endpoint(state, principal, request).await
            }
            _ => error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "GET or POST",
            ),
        };
    }
    if let Some(rest) = path.strip_prefix("/v1/admin/client-instances/")
        && !rest.is_empty()
        && !rest.contains('/')
    {
        return match *method {
            Method::GET => handle_admin_client_instance_detail(state, rest),
            Method::PUT => {
                handle_admin_client_instance_update_endpoint(state, principal, rest, request).await
            }
            Method::DELETE => handle_admin_client_instance_delete(state, principal, rest),
            _ => error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "GET, PUT, or DELETE",
            ),
        };
    }
    error_response(StatusCode::NOT_FOUND, "not_found", "unknown admin endpoint")
}

#[cfg(test)]
mod tests;
