//! Admin user CRUD: `/v1/admin/users/{list, detail, create, update,
//! reset_password, delete}`.
//!
//! Every mutation goes through [`mutate_collection`] so the
//! read-modify-write race against concurrent admin requests is closed
//! by the user-collection mutex and the uniqueness / last-user
//! invariants live in one place.

use std::sync::Arc;

use anyhow::Result;
use axum::Json;
use axum::extract::{Path, State};
use tracing::warn;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use underwrap_protocol::service::{
    AdminOkUserResponse, AdminUserCreateRequest, AdminUserDetailResponse, AdminUserListItem,
    AdminUserResetPasswordRequest, AdminUserUpdateRequest, AdminUsersListResponse,
    ServiceErrorResponse,
};

use crate::admin_api::AdminError;
use crate::admin_api::audit::log_admin_event;
use crate::http::responses::Body;
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
    actor: &Principal,
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

/// Map a `UserMutationError` to the audit reason + `AdminError` that
/// the user-CRUD handlers emit. Keeps every handler's failure path in
/// one place so the wire shape stays byte-for-byte consistent.
fn user_mutation_response(err: &UserMutationError) -> (Option<&'static str>, AdminError) {
    match err {
        UserMutationError::NotFound(_) => {
            (Some("not-found"), AdminError::not_found("unknown user"))
        }
        UserMutationError::DuplicateUsername(_) => (
            Some("already-exists"),
            AdminError::conflict("username_already_exists", err.to_string()),
        ),
        UserMutationError::LastUserProtected => (
            Some("last-user-protected"),
            AdminError::conflict(
                "last_user_protected",
                "cannot delete the last remaining user",
            ),
        ),
        UserMutationError::InvalidUsername | UserMutationError::BadHash => (
            None,
            AdminError::bad_request("invalid_user", err.to_string()),
        ),
    }
}

/// Convert an `anyhow::Error` raised by a `mutate_collection` closure
/// into the right `AdminError` + audit reason. Anything that wasn't a
/// `UserMutationError` falls through to a 400 `invalid_user` envelope
/// — matching the legacy behaviour for unexpected validation errors.
fn classify_mutation_error(err: anyhow::Error) -> (Option<&'static str>, AdminError) {
    match err.downcast_ref::<UserMutationError>() {
        Some(mut_err) => user_mutation_response(mut_err),
        None => (
            None,
            AdminError::bad_request("invalid_user", err.to_string()),
        ),
    }
}

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_users_list, handle_admin_user_create))
        .routes(routes!(
            handle_admin_user_detail,
            handle_admin_user_update,
            handle_admin_user_delete
        ))
        .routes(routes!(handle_admin_user_reset_password))
}

#[utoipa::path(
    get,
    path = "/v1/admin/users",
    tag = "users",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "List of every operator in the collection.", body = AdminUsersListResponse),
        (status = 500, description = "Backend could not read the operator collection.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_users_list(
    State(state): State<Arc<AdminRuntimeState>>,
) -> Result<Json<AdminUsersListResponse>, AdminError> {
    let collection = crate::users::load(&state.users_file).map_err(|e| {
        warn!(err = %e, "users.list: collection unreadable");
        AdminError::internal_with_code("internal", "user collection unavailable")
    })?;
    let mut users: Vec<AdminUserListItem> = collection.users.iter().map(user_list_item).collect();
    users.sort_by(|a, b| a.username.cmp(&b.username));
    Ok(Json(AdminUsersListResponse { users }))
}

#[utoipa::path(
    get,
    path = "/v1/admin/users/{id}",
    tag = "users",
    security(("session_cookie" = [])),
    params(("id" = String, Path, description = "Operator id.")),
    responses(
        (status = 200, description = "Operator record.", body = AdminUserDetailResponse),
        (status = 404, description = "Operator not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_user_detail(
    State(state): State<Arc<AdminRuntimeState>>,
    Path(id): Path<String>,
) -> Result<Json<AdminUserDetailResponse>, AdminError> {
    let collection = crate::users::load(&state.users_file).map_err(|e| {
        warn!(err = %e, "users.detail: collection unreadable");
        AdminError::internal_with_code("internal", "user collection unavailable")
    })?;
    let record = collection
        .find_by_id(&id)
        .ok_or_else(|| AdminError::not_found("unknown user"))?;
    Ok(Json(user_detail(record)))
}

#[utoipa::path(
    post,
    path = "/v1/admin/users",
    tag = "users",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminUserCreateRequest,
    responses(
        (status = 200, description = "Operator created.", body = AdminUserDetailResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 409, description = "Username already exists.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_user_create(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminUserCreateRequest>,
) -> Result<Json<AdminUserDetailResponse>, AdminError> {
    if body.password.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_password",
            "password must not be empty",
        ));
    }
    let username_norm = crate::users::normalise_username(&body.username);
    if username_norm.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_username",
            "username must not be empty",
        ));
    }
    let password_hash = crate::auth::hash_password(&body.password).map_err(|e| {
        warn!(err = %e, "users.create: argon2 hash failed");
        AdminError::internal_with_code("internal", "could not hash password")
    })?;
    let now = crate::users::now_epoch_secs();
    let record = crate::users::UserRecord {
        id: crate::users::new_user_id(),
        username: username_norm.clone(),
        password_hash,
        created_at: now,
        password_changed_at: now,
    };
    let target_username = record.username.clone();
    let result = mutate_collection(&state, |c| {
        c.add(record.clone())?;
        Ok(record)
    });
    match result {
        Ok(saved) => {
            write_admin_user_audit(&principal, &target_username, "user.create", None, "success");
            Ok(Json(user_detail(&saved)))
        }
        Err(e) => {
            let (reason, response) = classify_mutation_error(e);
            write_admin_user_audit(
                &principal,
                &target_username,
                "user.create",
                reason,
                "failure",
            );
            Err(response)
        }
    }
}

#[utoipa::path(
    put,
    path = "/v1/admin/users/{id}",
    tag = "users",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Operator id.")),
    request_body = AdminUserUpdateRequest,
    responses(
        (status = 200, description = "Operator updated.", body = AdminUserDetailResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Operator not found.", body = ServiceErrorResponse),
        (status = 409, description = "Username collision.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_user_update(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
    Body(body): Body<AdminUserUpdateRequest>,
) -> Result<Json<AdminUserDetailResponse>, AdminError> {
    let result = mutate_collection(&state, |c| {
        let updated = c.update(&id, |u| {
            if let Some(username) = &body.username {
                u.username = username.clone();
            }
        })?;
        Ok(updated.clone())
    });
    match result {
        Ok(updated) => {
            write_admin_user_audit(
                &principal,
                &updated.username,
                "user.update",
                None,
                "success",
            );
            Ok(Json(user_detail(&updated)))
        }
        Err(e) => {
            let (reason, response) = classify_mutation_error(e);
            write_admin_user_audit(&principal, &id, "user.update", reason, "failure");
            Err(response)
        }
    }
}

#[utoipa::path(
    post,
    path = "/v1/admin/users/{id}/password",
    tag = "users",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Operator id.")),
    request_body = AdminUserResetPasswordRequest,
    responses(
        (status = 200, description = "Password reset; the target user's sessions are invalidated automatically.", body = AdminOkUserResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Operator not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_user_reset_password(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
    Body(body): Body<AdminUserResetPasswordRequest>,
) -> Result<Json<AdminOkUserResponse>, AdminError> {
    if body.password.is_empty() {
        return Err(AdminError::bad_request(
            "invalid_password",
            "password must not be empty",
        ));
    }
    let new_hash = crate::auth::hash_password(&body.password).map_err(|e| {
        warn!(err = %e, "users.reset-password: argon2 hash failed");
        AdminError::internal_with_code("internal", "could not hash password")
    })?;
    let now = crate::users::now_epoch_secs();
    let result = mutate_collection(&state, |c| {
        let updated = c.update(&id, |u| {
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
                &principal,
                &target_username,
                "user.password.reset",
                None,
                "success",
            );
            Ok(Json(AdminOkUserResponse { ok: true }))
        }
        Err(e) => {
            let (reason, response) = classify_mutation_error(e);
            write_admin_user_audit(&principal, &id, "user.password.reset", reason, "failure");
            Err(response)
        }
    }
}

#[utoipa::path(
    delete,
    path = "/v1/admin/users/{id}",
    tag = "users",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Operator id.")),
    responses(
        (status = 200, description = "Operator deleted; all sessions held by the deleted user are invalidated automatically.", body = AdminOkUserResponse),
        (status = 400, description = "Cannot delete the last operator.", body = ServiceErrorResponse),
        (status = 404, description = "Operator not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_user_delete(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
) -> Result<Json<AdminOkUserResponse>, AdminError> {
    let result = mutate_collection(&state, |c| Ok(c.remove(&id)?));
    match result {
        Ok(removed) => {
            // Per-user session revocation is automatic: once the
            // record is gone, `AuthBackend::get_user` returns None on
            // any subsequent request, `auth_session.user` is None,
            // and the `require_login_layer` rejects with 401.
            write_admin_user_audit(
                &principal,
                &removed.username,
                "user.delete",
                None,
                "success",
            );
            Ok(Json(AdminOkUserResponse { ok: true }))
        }
        Err(e) => {
            let (reason, response) = classify_mutation_error(e);
            write_admin_user_audit(&principal, &id, "user.delete", reason, "failure");
            Err(response)
        }
    }
}

#[cfg(test)]
mod tests;
