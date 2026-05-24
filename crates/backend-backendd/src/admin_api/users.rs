//! Admin user CRUD: `/v1/admin/users/{list, create, update,
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
    AdminOkUserResponse, AdminUserCreateRequest, AdminUserListItem, AdminUserResetPasswordRequest,
    AdminUserUpdateRequest, AdminUsersListResponse, ServiceErrorResponse,
};

use crate::admin_api::AdminError;
use crate::admin_api::audit::{AuditEntity, log_admin_event};
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

/// Read-modify-write on the user collection, serialised across
/// concurrent admin requests via `user_collection_lock`. The closure
/// may mutate the collection in-place; on `Ok(_)` we diff the before
/// and after collections and emit one per-record save or delete for
/// each changed user. On `Err(_)` nothing is persisted.
///
/// The closure returns `anyhow::Result<T>` so user-level mutation
/// errors flow through via `From<UserMutationError> for anyhow::Error`
/// (auto-derived by `thiserror`); handlers then `downcast_ref` to
/// match on the variant.
pub(crate) fn mutate_collection<T, F>(state: &AdminRuntimeState, f: F) -> Result<T>
where
    F: FnOnce(&mut crate::users::UserCollection) -> Result<T>,
{
    use std::collections::HashMap;
    let _guard = state.user_collection_lock.lock().unwrap();
    let before = crate::users::load(&state.users_dir)?;
    let mut after = before.clone();
    let out = f(&mut after)?;

    let before_by_id: HashMap<&str, &crate::users::UserRecord> =
        before.users.iter().map(|u| (u.id.as_str(), u)).collect();
    let after_by_id: HashMap<&str, &crate::users::UserRecord> =
        after.users.iter().map(|u| (u.id.as_str(), u)).collect();

    for old in &before.users {
        if !after_by_id.contains_key(old.id.as_str()) {
            crate::users::delete_user(&state.users_dir, &old.id)?;
        }
    }
    for new in &after.users {
        if before_by_id.get(new.id.as_str()).copied() != Some(new) {
            crate::users::save_user(&state.users_dir, new)?;
        }
    }
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
/// — unexpected validation errors are surfaced as bad requests so the
/// operator gets the underlying message instead of a generic 500.
fn classify_mutation_error(err: anyhow::Error) -> (Option<&'static str>, AdminError) {
    match err.downcast_ref::<UserMutationError>() {
        Some(mut_err) => user_mutation_response(mut_err),
        None => (
            None,
            AdminError::bad_request("invalid_user", err.to_string()),
        ),
    }
}

/// Audit shim for the user-CRUD handlers. On `Ok(_)`, log a success
/// event keyed by the username derived from the value; on `Err(_)`,
/// classify the error into an `AdminError` + audit reason and log a
/// failure event keyed by `failure_username` (typically the request's
/// path-id or the create body's normalised name, since the value we
/// could have inspected is gone with the error).
fn audit_user_mutation<T>(
    action: &str,
    principal: &Principal,
    failure_username: &str,
    result: Result<T>,
    success_username: impl FnOnce(&T) -> &str,
) -> Result<T, AdminError> {
    match result {
        Ok(value) => {
            log_admin_event(
                action,
                principal,
                AuditEntity::User {
                    username: success_username(&value),
                },
                None,
                "success",
            );
            Ok(value)
        }
        Err(e) => {
            let (reason, response) = classify_mutation_error(e);
            log_admin_event(
                action,
                principal,
                AuditEntity::User {
                    username: failure_username,
                },
                reason,
                "failure",
            );
            Err(response)
        }
    }
}

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<AdminRuntimeState>> {
    OpenApiRouter::new()
        .routes(routes!(handle_admin_users_list, handle_admin_user_create))
        .routes(routes!(handle_admin_user_update, handle_admin_user_delete))
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
    let collection = crate::users::load(&state.users_dir).map_err(|e| {
        warn!(err = %e, "users.list: collection unreadable");
        AdminError::internal_with_code("internal", "user collection unavailable")
    })?;
    let mut users: Vec<AdminUserListItem> = collection.users.iter().map(user_list_item).collect();
    users.sort_by(|a, b| a.username.cmp(&b.username));
    Ok(Json(AdminUsersListResponse { users }))
}

#[utoipa::path(
    post,
    path = "/v1/admin/users",
    tag = "users",
    security(("session_cookie" = []), ("csrf_header" = [])),
    request_body = AdminUserCreateRequest,
    responses(
        (status = 200, description = "Operator created.", body = AdminUserListItem),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 409, description = "Username already exists.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_user_create(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Body(body): Body<AdminUserCreateRequest>,
) -> Result<Json<AdminUserListItem>, AdminError> {
    crate::admin_api::util::validate_password_or_bad_request(&body.password)?;
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
    let saved = audit_user_mutation("user.create", &principal, &target_username, result, |r| {
        &r.username
    })?;
    Ok(Json(user_list_item(&saved)))
}

#[utoipa::path(
    put,
    path = "/v1/admin/users/{id}",
    tag = "users",
    security(("session_cookie" = []), ("csrf_header" = [])),
    params(("id" = String, Path, description = "Operator id.")),
    request_body = AdminUserUpdateRequest,
    responses(
        (status = 200, description = "Operator updated.", body = AdminUserListItem),
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
) -> Result<Json<AdminUserListItem>, AdminError> {
    let result = mutate_collection(&state, |c| {
        let updated = c.update(&id, |u| {
            if let Some(username) = &body.username {
                u.username = username.clone();
            }
        })?;
        Ok(updated.clone())
    });
    let updated = audit_user_mutation("user.update", &principal, &id, result, |u| &u.username)?;
    Ok(Json(user_list_item(&updated)))
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
    crate::admin_api::util::validate_password_or_bad_request(&body.password)?;
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
    // The target operator's existing sessions are invalidated
    // automatically: `axum_login` records the password-hash bytes in
    // each session and re-checks them on every request via
    // `session_auth_hash()`. Once we write the new hash here, the next
    // request that arrives carrying any pre-rotation session for this
    // user fails the hash check and the session is flushed
    // server-side.
    audit_user_mutation("user.password.reset", &principal, &id, result, |s| {
        s.as_str()
    })?;
    Ok(Json(AdminOkUserResponse { ok: true }))
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
    // Per-user session revocation is automatic: once the record is
    // gone, `AuthBackend::get_user` returns None on any subsequent
    // request, `auth_session.user` is None, and the
    // `require_login_layer` rejects with 401.
    audit_user_mutation("user.delete", &principal, &id, result, |r| &r.username)?;
    Ok(Json(AdminOkUserResponse { ok: true }))
}

#[cfg(test)]
mod tests;
