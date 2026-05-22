//! Wrapper / resource pair admin CRUD:
//! `/v1/admin/pairs/{list, create, delete}`.

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};

use crate::http::responses::parse_json_body;

use crate::admin_api::audit::log_admin_event;
use crate::admin_api::stores::admin_store;
use crate::http::responses::{error_response, json_response};
use crate::runtime_state::AdminRuntimeState;
use crate::storage::documents::{
    ENTITY_DEPENDENT_SAMPLE_LIMIT, load_pairs_document, load_policy_document,
    load_resources_document, save_pairs_document,
};

pub(crate) async fn handle_admin_pairs_list_endpoint(
    state: &AdminRuntimeState,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<underwrap_protocol::service::AdminPairsListRequest>(request)
        .await
    {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    handle_admin_pairs_list(state, &body)
}

pub(crate) async fn handle_admin_pair_create_endpoint(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<underwrap_protocol::service::AdminPairCreateRequest>(request)
        .await
    {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    handle_admin_pair_create(state, principal, &body)
}

pub(crate) async fn handle_admin_pair_delete_endpoint(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<underwrap_protocol::service::AdminPairDeleteRequest>(request)
        .await
    {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    handle_admin_pair_delete(state, principal, &body)
}

pub(crate) fn handle_admin_pairs_list(
    state: &AdminRuntimeState,
    body: &underwrap_protocol::service::AdminPairsListRequest,
) -> Response<Full<Bytes>> {
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    match load_pairs_document(store.as_ref()) {
        Ok(mut doc) => {
            doc.pairs.sort_by(|a, b| {
                a.wrapper
                    .cmp(&b.wrapper)
                    .then(a.resource_id.cmp(&b.resource_id))
            });
            json_response(
                StatusCode::OK,
                &underwrap_protocol::service::AdminPairsListResponse { pairs: doc.pairs },
            )
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("loading pairs document: {e:#}"),
        ),
    }
}

pub(crate) fn handle_admin_pair_create(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: &underwrap_protocol::service::AdminPairCreateRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::entities::{EntityCreator, WrapperResourcePairRecord};
    let wrapper = body.wrapper.trim();
    let resource_id = body.resource_id.trim();
    if wrapper.is_empty() || resource_id.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_pair",
            "wrapper and resource_id must both be non-empty",
        );
    }
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let resources = match load_resources_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading resources document: {e:#}"),
            );
        }
    };
    if resources.find_by_id(resource_id).is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "unknown_resource",
            &format!("resource id '{resource_id}' must exist before a pair can reference it"),
        );
    }
    let mut pairs = match load_pairs_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading pairs document: {e:#}"),
            );
        }
    };
    if pairs.find(wrapper, resource_id).is_some() {
        return error_response(
            StatusCode::CONFLICT,
            "pair_already_exists",
            &format!("pair ({wrapper}, {resource_id}) already exists"),
        );
    }
    let record = WrapperResourcePairRecord {
        wrapper: wrapper.to_string(),
        resource_id: resource_id.to_string(),
        created_by: EntityCreator::Operator {
            id: principal.id.clone(),
            username: principal.username.clone(),
        },
        created_at: crate::users::now_epoch_secs(),
        last_used_at: None,
    };
    pairs
        .insert(record.clone())
        .expect("find returned None just above");
    if let Err(e) = save_pairs_document(store.as_ref(), &pairs) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting pairs document: {e:#}"),
        );
    }
    log_admin_event(
        "pair.create",
        &principal.username,
        Some(wrapper),
        Some(resource_id),
        None,
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &underwrap_protocol::service::AdminPairCreateResponse { pair: record },
    )
}

pub(crate) fn handle_admin_pair_delete(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: &underwrap_protocol::service::AdminPairDeleteRequest,
) -> Response<Full<Bytes>> {
    use underwrap_protocol::service::{
        AdminPairDeleteBlockers, AdminPairDeleteResponse, AdminWrapperPermissionKey,
    };
    let store = match admin_store(state, &body.store) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let mut doc = match load_pairs_document(store.as_ref()) {
        Ok(d) => d,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                &format!("loading pairs document: {e:#}"),
            );
        }
    };
    if doc.find(&body.wrapper, &body.resource_id).is_none() {
        return error_response(StatusCode::NOT_FOUND, "not_found", "unknown pair");
    }

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
        .filter(|p| p.wrapper == body.wrapper && p.resource_id == body.resource_id)
        .map(|p| AdminWrapperPermissionKey {
            client_instance: p.client_instance.clone(),
            wrapper: p.wrapper.clone(),
            resource_id: p.resource_id.clone(),
        })
        .collect();
    let permission_count = u32::try_from(dependent_permissions.len()).unwrap_or(u32::MAX);
    let mut sample_permissions = dependent_permissions;
    sample_permissions.truncate(ENTITY_DEPENDENT_SAMPLE_LIMIT);

    if permission_count > 0 {
        log_admin_event(
            "pair.delete",
            &principal.username,
            Some(&body.wrapper),
            Some(&body.resource_id),
            None,
            None,
            "blocked",
        );
        return json_response(
            StatusCode::OK,
            &AdminPairDeleteResponse {
                ok: false,
                blocked_by: Some(AdminPairDeleteBlockers {
                    permission_count,
                    sample_permissions,
                }),
            },
        );
    }

    doc.remove(&body.wrapper, &body.resource_id);
    if let Err(e) = save_pairs_document(store.as_ref(), &doc) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            &format!("persisting pairs document: {e:#}"),
        );
    }
    log_admin_event(
        "pair.delete",
        &principal.username,
        Some(&body.wrapper),
        Some(&body.resource_id),
        None,
        None,
        "success",
    );
    json_response(
        StatusCode::OK,
        &AdminPairDeleteResponse {
            ok: true,
            blocked_by: None,
        },
    )
}

pub(crate) async fn route(
    method: &Method,
    path: &str,
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    match (method, path) {
        (&Method::POST, "/v1/admin/pairs/list") => {
            handle_admin_pairs_list_endpoint(state, request).await
        }
        (&Method::POST, "/v1/admin/pairs/create") => {
            handle_admin_pair_create_endpoint(state, principal, request).await
        }
        (&Method::POST, "/v1/admin/pairs/delete") => {
            handle_admin_pair_delete_endpoint(state, principal, request).await
        }
        _ => error_response(StatusCode::NOT_FOUND, "not_found", "unknown admin endpoint"),
    }
}

#[cfg(test)]
mod tests;
