//! Admin user CRUD: `/v1/admin/users/{list, detail, create, update,
//! reset_password, delete}`.
//!
//! Every mutation goes through [`mutate_collection`] so the
//! read-modify-write race against concurrent admin requests is closed
//! by the user-collection mutex and the uniqueness / last-user
//! invariants live in one place.

use anyhow::Result;
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use tracing::warn;

use underwrap_protocol::service::{
    AdminOkUserResponse, AdminUserCreateRequest, AdminUserDetailResponse, AdminUserListItem,
    AdminUserResetPasswordRequest, AdminUserUpdateRequest, AdminUsersListResponse,
};

use crate::admin_api::audit::log_admin_event;
use crate::http::responses::{error_response, json_response, parse_json_body};
use crate::runtime_state::AdminRuntimeState;

pub(crate) fn user_list_item(record: &crate::users::UserRecord) -> AdminUserListItem {
    AdminUserListItem {
        id: record.id.clone(),
        username: record.username.clone(),
        created_at: record.created_at,
        password_changed_at: record.password_changed_at,
    }
}

pub(crate) fn user_detail(record: &crate::users::UserRecord) -> AdminUserDetailResponse {
    AdminUserDetailResponse {
        id: record.id.clone(),
        username: record.username.clone(),
        created_at: record.created_at,
        password_changed_at: record.password_changed_at,
    }
}

pub(crate) fn write_admin_user_audit(
    _state: &AdminRuntimeState,
    actor: &crate::users::Principal,
    target_username: &str,
    action: &str,
    result: &str,
) {
    let resource = format!("user:{target_username}");
    log_admin_event(
        action,
        &actor.username,
        None,
        Some(resource.as_str()),
        None,
        None,
        result,
    );
}

/// Read-modify-write on the user collection, serialised across
/// concurrent admin requests via `user_collection_lock`. The closure
/// may mutate the collection in-place; on `Ok(_)` we persist and
/// return whatever the closure returned. On `Err(_)` the collection is
/// *not* persisted.
pub(crate) fn mutate_collection<T, F>(state: &AdminRuntimeState, f: F) -> Result<T>
where
    F: FnOnce(&mut crate::users::UserCollection) -> Result<T>,
{
    let _guard = state.user_collection_lock.lock().unwrap();
    let mut collection = crate::users::load(&state.users_file)?;
    let out = f(&mut collection)?;
    crate::users::save(&state.users_file, &collection)?;
    Ok(out)
}

#[allow(clippy::result_large_err)]
pub(crate) fn handle_admin_users_list(state: &AdminRuntimeState) -> Response<Full<Bytes>> {
    let collection = match crate::users::load(&state.users_file) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "users.list: collection unreadable");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "user collection unavailable",
            );
        }
    };
    let mut users: Vec<AdminUserListItem> = collection.users.iter().map(user_list_item).collect();
    users.sort_by(|a, b| a.username.cmp(&b.username));
    json_response(StatusCode::OK, &AdminUsersListResponse { users })
}

#[allow(clippy::result_large_err)]
pub(crate) fn handle_admin_user_detail(
    state: &AdminRuntimeState,
    id: &str,
) -> Response<Full<Bytes>> {
    let collection = match crate::users::load(&state.users_file) {
        Ok(c) => c,
        Err(e) => {
            warn!(err = %e, "users.detail: collection unreadable");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "user collection unavailable",
            );
        }
    };
    match collection.find_by_id(id) {
        Some(record) => json_response(StatusCode::OK, &user_detail(record)),
        None => error_response(StatusCode::NOT_FOUND, "not_found", "unknown user"),
    }
}

#[allow(clippy::result_large_err)]
pub(crate) async fn handle_admin_user_create(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminUserCreateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_user_create(state, principal, body)
}

#[allow(clippy::result_large_err)]
pub(crate) fn do_admin_user_create(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: AdminUserCreateRequest,
) -> Response<Full<Bytes>> {
    if body.password.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_password",
            "password must not be empty",
        );
    }
    let username_norm = crate::users::normalise_username(&body.username);
    if username_norm.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_username",
            "username must not be empty",
        );
    }
    let password_hash = match crate::auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => {
            warn!(err = %e, "users.create: argon2 hash failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not hash password",
            );
        }
    };
    let now = crate::users::now_epoch_secs();
    let record = crate::users::UserRecord {
        id: crate::users::new_user_id(),
        username: username_norm.clone(),
        password_hash,
        created_at: now,
        password_changed_at: now,
    };
    let target_username = record.username.clone();
    let result = mutate_collection(state, |c| {
        c.add(record.clone())?;
        Ok(record)
    });
    match result {
        Ok(saved) => {
            write_admin_user_audit(state, principal, &target_username, "user.create", "success");
            json_response(StatusCode::OK, &user_detail(&saved))
        }
        Err(e) => {
            let msg = e.to_string();
            write_admin_user_audit(state, principal, &target_username, "user.create", "failure");
            if msg.contains("already exists") {
                error_response(StatusCode::CONFLICT, "username_already_exists", &msg)
            } else {
                error_response(StatusCode::BAD_REQUEST, "invalid_user", &msg)
            }
        }
    }
}

#[allow(clippy::result_large_err)]
pub(crate) async fn handle_admin_user_update(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    id: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminUserUpdateRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    do_admin_user_update(state, principal, id, body)
}

#[allow(clippy::result_large_err)]
pub(crate) fn do_admin_user_update(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    id: &str,
    body: AdminUserUpdateRequest,
) -> Response<Full<Bytes>> {
    let id_owned = id.to_string();
    let result = mutate_collection(state, |c| {
        if c.find_by_id(&id_owned).is_none() {
            anyhow::bail!("not_found");
        }
        let updated = c.update(&id_owned, |u| {
            if let Some(username) = &body.username {
                u.username = username.clone();
            }
        })?;
        Ok(updated.clone())
    });
    match result {
        Ok(updated) => {
            write_admin_user_audit(
                state,
                principal,
                &updated.username,
                "user.update",
                "success",
            );
            json_response(StatusCode::OK, &user_detail(&updated))
        }
        Err(e) => {
            let msg = e.to_string();
            write_admin_user_audit(state, principal, &id_owned, "user.update", "failure");
            if msg.contains("not_found") || msg.contains("not found") {
                error_response(StatusCode::NOT_FOUND, "not_found", "unknown user")
            } else if msg.contains("already exists") {
                error_response(StatusCode::CONFLICT, "username_already_exists", &msg)
            } else {
                error_response(StatusCode::BAD_REQUEST, "invalid_user", &msg)
            }
        }
    }
}

#[allow(clippy::result_large_err)]
pub(crate) async fn handle_admin_user_reset_password(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    id: &str,
    request: Request<Incoming>,
) -> Response<Full<Bytes>> {
    let body = match parse_json_body::<AdminUserResetPasswordRequest>(request).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    if body.password.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_password",
            "password must not be empty",
        );
    }
    let new_hash = match crate::auth::hash_password(&body.password) {
        Ok(h) => h,
        Err(e) => {
            warn!(err = %e, "users.reset-password: argon2 hash failed");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "could not hash password",
            );
        }
    };
    let id_owned = id.to_string();
    let now = crate::users::now_epoch_secs();
    let result = mutate_collection(state, |c| {
        if c.find_by_id(&id_owned).is_none() {
            anyhow::bail!("not_found");
        }
        let updated = c.update(&id_owned, |u| {
            u.password_hash = new_hash.clone();
            u.password_changed_at = now;
        })?;
        Ok(updated.username.clone())
    });
    match result {
        Ok(target_username) => {
            state
                .session_revocations
                .revoke_username(&target_username, now);
            write_admin_user_audit(
                state,
                principal,
                &target_username,
                "user.password.reset",
                "success",
            );
            json_response(StatusCode::OK, &AdminOkUserResponse { ok: true })
        }
        Err(e) => {
            let msg = e.to_string();
            write_admin_user_audit(
                state,
                principal,
                &id_owned,
                "user.password.reset",
                "failure",
            );
            if msg.contains("not_found") {
                error_response(StatusCode::NOT_FOUND, "not_found", "unknown user")
            } else {
                error_response(StatusCode::BAD_REQUEST, "invalid_user", &msg)
            }
        }
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn handle_admin_user_delete(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    id: &str,
) -> Response<Full<Bytes>> {
    let id_owned = id.to_string();
    let now = crate::users::now_epoch_secs();
    let result = mutate_collection(state, |c| {
        if c.find_by_id(&id_owned).is_none() {
            anyhow::bail!("not_found");
        }
        c.remove(&id_owned)
    });
    match result {
        Ok(removed) => {
            state
                .session_revocations
                .revoke_username(&removed.username, now);
            write_admin_user_audit(
                state,
                principal,
                &removed.username,
                "user.delete",
                "success",
            );
            json_response(StatusCode::OK, &AdminOkUserResponse { ok: true })
        }
        Err(e) => {
            let msg = e.to_string();
            write_admin_user_audit(state, principal, &id_owned, "user.delete", "failure");
            if msg.contains("not_found") {
                error_response(StatusCode::NOT_FOUND, "not_found", "unknown user")
            } else if msg.contains("last_user_protected") {
                error_response(
                    StatusCode::CONFLICT,
                    "last_user_protected",
                    "cannot delete the last remaining user",
                )
            } else {
                error_response(StatusCode::BAD_REQUEST, "invalid_user", &msg)
            }
        }
    }
}
