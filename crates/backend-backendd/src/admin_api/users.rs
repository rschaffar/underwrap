//! Admin user CRUD: `/v1/admin/users/{list, detail, create, update,
//! reset_password, delete}`.
//!
//! Every mutation goes through [`mutate_collection`] so the
//! read-modify-write race against concurrent admin requests is closed
//! by the user-collection mutex and the uniqueness / last-user
//! invariants live in one place.

use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::get;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};
use tracing::warn;

use underwrap_protocol::service::{
    AdminOkUserResponse, AdminUserCreateRequest, AdminUserDetailResponse, AdminUserListItem,
    AdminUserResetPasswordRequest, AdminUserUpdateRequest, AdminUsersListResponse,
};

use crate::admin_api::audit::log_admin_event;
use crate::http::responses::{Body, error_response, json_response};
use crate::runtime_state::AdminRuntimeState;
use crate::users::{Principal, UserMutationError};

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
    actor: &crate::users::Principal,
    target_username: &str,
    action: &str,
    operation: Option<&str>,
    result: &str,
) {
    let resource = format!("user:{target_username}");
    log_admin_event(
        action,
        &actor.username,
        None,
        Some(resource.as_str()),
        None,
        operation,
        result,
    );
}

/// Read-modify-write on the user collection, serialised across
/// concurrent admin requests via `user_collection_lock`. The closure
/// may mutate the collection in-place; on `Ok(_)` we persist and
/// return whatever the closure returned. On `Err(_)` the collection is
/// *not* persisted.
///
/// The closure returns `anyhow::Result<T>` so user-level mutation
/// errors flow through via `From<UserMutationError> for anyhow::Error`
/// (auto-derived by `thiserror`); handlers then `downcast_ref` to
/// match on the variant.
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

/// Map a `UserMutationError` to the audit reason + HTTP response
/// that the user-CRUD handlers emit. Keeps every handler's failure
/// path in one place so the wire shape stays byte-for-byte consistent.
fn user_mutation_response(
    err: &UserMutationError,
) -> (Option<&'static str>, Response<Full<Bytes>>) {
    match err {
        UserMutationError::NotFound(_) => (
            Some("not-found"),
            error_response(StatusCode::NOT_FOUND, "not_found", "unknown user"),
        ),
        UserMutationError::DuplicateUsername(_) => (
            Some("already-exists"),
            error_response(
                StatusCode::CONFLICT,
                "username_already_exists",
                &err.to_string(),
            ),
        ),
        UserMutationError::LastUserProtected => (
            Some("last-user-protected"),
            error_response(
                StatusCode::CONFLICT,
                "last_user_protected",
                "cannot delete the last remaining user",
            ),
        ),
        UserMutationError::InvalidUsername | UserMutationError::BadHash => (
            None,
            error_response(StatusCode::BAD_REQUEST, "invalid_user", &err.to_string()),
        ),
    }
}

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

pub(crate) fn handle_admin_user_create(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    body: &AdminUserCreateRequest,
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
            write_admin_user_audit(principal, &target_username, "user.create", None, "success");
            json_response(StatusCode::OK, &user_detail(&saved))
        }
        Err(e) => {
            let (reason, response) = match e.downcast_ref::<UserMutationError>() {
                Some(mut_err) => user_mutation_response(mut_err),
                None => (
                    None,
                    error_response(StatusCode::BAD_REQUEST, "invalid_user", &e.to_string()),
                ),
            };
            write_admin_user_audit(
                principal,
                &target_username,
                "user.create",
                reason,
                "failure",
            );
            response
        }
    }
}

pub(crate) fn handle_admin_user_update(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    id: &str,
    body: &AdminUserUpdateRequest,
) -> Response<Full<Bytes>> {
    let id_owned = id.to_string();
    let result = mutate_collection(state, |c| {
        let updated = c.update(&id_owned, |u| {
            if let Some(username) = &body.username {
                u.username = username.clone();
            }
        })?;
        Ok(updated.clone())
    });
    match result {
        Ok(updated) => {
            write_admin_user_audit(principal, &updated.username, "user.update", None, "success");
            json_response(StatusCode::OK, &user_detail(&updated))
        }
        Err(e) => {
            let (reason, response) = match e.downcast_ref::<UserMutationError>() {
                Some(mut_err) => user_mutation_response(mut_err),
                None => (
                    None,
                    error_response(StatusCode::BAD_REQUEST, "invalid_user", &e.to_string()),
                ),
            };
            write_admin_user_audit(principal, &id_owned, "user.update", reason, "failure");
            response
        }
    }
}

pub(crate) fn handle_admin_user_reset_password(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    id: &str,
    body: &AdminUserResetPasswordRequest,
) -> Response<Full<Bytes>> {
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
        let updated = c.update(&id_owned, |u| {
            u.password_hash = new_hash.clone();
            u.password_changed_at = now;
        })?;
        Ok(updated.username.clone())
    });
    match result {
        Ok(target_username) => {
            // The target operator's existing sessions are invalidated
            // automatically: `axum_login` records the password-hash
            // bytes in each session and re-checks them on every
            // request via `session_auth_hash()`. Once we write the
            // new hash here, the next request that arrives carrying
            // any pre-rotation session for this user fails the
            // hash check and the session is flushed server-side.
            write_admin_user_audit(
                principal,
                &target_username,
                "user.password.reset",
                None,
                "success",
            );
            json_response(StatusCode::OK, &AdminOkUserResponse { ok: true })
        }
        Err(e) => {
            let (reason, response) = match e.downcast_ref::<UserMutationError>() {
                Some(mut_err) => user_mutation_response(mut_err),
                None => (
                    None,
                    error_response(StatusCode::BAD_REQUEST, "invalid_user", &e.to_string()),
                ),
            };
            write_admin_user_audit(
                principal,
                &id_owned,
                "user.password.reset",
                reason,
                "failure",
            );
            response
        }
    }
}

pub(crate) fn handle_admin_user_delete(
    state: &AdminRuntimeState,
    principal: &crate::users::Principal,
    id: &str,
) -> Response<Full<Bytes>> {
    let id_owned = id.to_string();
    let result = mutate_collection(state, |c| Ok(c.remove(&id_owned)?));
    match result {
        Ok(removed) => {
            // Per-user session revocation is automatic: once the
            // record is gone, `AuthBackend::get_user` returns None on
            // any subsequent request, `auth_session.user` is None,
            // and the `require_login_layer` rejects with 401.
            write_admin_user_audit(principal, &removed.username, "user.delete", None, "success");
            json_response(StatusCode::OK, &AdminOkUserResponse { ok: true })
        }
        Err(e) => {
            let (reason, response) = match e.downcast_ref::<UserMutationError>() {
                Some(mut_err) => user_mutation_response(mut_err),
                None => (
                    None,
                    error_response(StatusCode::BAD_REQUEST, "invalid_user", &e.to_string()),
                ),
            };
            write_admin_user_audit(principal, &id_owned, "user.delete", reason, "failure");
            response
        }
    }
}

pub(crate) fn router() -> Router<Arc<AdminRuntimeState>> {
    Router::new()
        .route(
            "/v1/admin/users",
            get(axum_users_list).post(axum_user_create),
        )
        .route(
            "/v1/admin/users/{id}",
            get(axum_user_detail)
                .put(axum_user_update)
                .delete(axum_user_delete),
        )
        .route(
            "/v1/admin/users/{id}/password",
            axum::routing::post(axum_user_reset_password),
        )
}

async fn axum_users_list(State(state): State<Arc<AdminRuntimeState>>) -> Response<Full<Bytes>> {
    handle_admin_users_list(&state)
}

async fn axum_user_create(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(parsed): Body<AdminUserCreateRequest>,
) -> Response<Full<Bytes>> {
    handle_admin_user_create(&state, &principal, &parsed)
}

async fn axum_user_detail(
    State(state): State<Arc<AdminRuntimeState>>,
    Path(id): Path<String>,
) -> Response<Full<Bytes>> {
    handle_admin_user_detail(&state, &id)
}

async fn axum_user_update(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
    Body(parsed): Body<AdminUserUpdateRequest>,
) -> Response<Full<Bytes>> {
    handle_admin_user_update(&state, &principal, &id, &parsed)
}

async fn axum_user_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
) -> Response<Full<Bytes>> {
    handle_admin_user_delete(&state, &principal, &id)
}

async fn axum_user_reset_password(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
    Body(parsed): Body<AdminUserResetPasswordRequest>,
) -> Response<Full<Bytes>> {
    handle_admin_user_reset_password(&state, &principal, &id, &parsed)
}

#[cfg(test)]
mod tests;
