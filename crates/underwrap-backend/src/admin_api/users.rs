//! Admin user CRUD: `/v1/admin/users/{list, create, update,
//! reset_password, delete}`.
//!
//! Every mutation goes through [`mutate_collection`] so the
//! read-modify-write race against concurrent admin requests is closed
//! by the user-collection mutex and the uniqueness / last-user
//! invariants live in one place.

use std::sync::Arc;

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
use crate::admin_api::csrf::Principal;
use crate::http::responses::Body;
use crate::runtime_state::ListenerState;
use crate::users::UserMutationError;

pub(crate) fn user_list_item(record: &crate::users::UserRecord) -> AdminUserListItem {
    AdminUserListItem {
        id: record.id.clone(),
        username: record.username.clone(),
        created_at: record.created_at,
        password_changed_at: record.password_changed_at,
    }
}

/// What [`mutate_collection`] should persist after the closure finishes.
/// Each handler names the one record it mutated, so persistence never needs to
/// infer the change by diffing the collection.
pub(crate) enum CollectionWrite {
    /// Persist the user record with this id (the closure just added
    /// or updated it).
    Save(String),
    /// Delete the user record with this id from disk (the closure
    /// just removed it from the collection).
    Delete(String),
}

/// Errors raised inside [`mutate_collection`]. The split lets handlers map
/// user-level invariant violations to typed 4xx responses and disk or
/// serialization failures to 500 responses.
#[derive(Debug)]
pub(crate) enum MutateCollectionError {
    User(UserMutationError),
    Io(anyhow::Error),
}

impl std::fmt::Display for MutateCollectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MutateCollectionError::User(e) => std::fmt::Display::fmt(e, f),
            MutateCollectionError::Io(e) => std::fmt::Display::fmt(e, f),
        }
    }
}

impl MutateCollectionError {
    fn into_admin_response(self) -> (Option<&'static str>, AdminError) {
        match self {
            MutateCollectionError::User(UserMutationError::NotFound(_)) => {
                (Some("not-found"), AdminError::not_found("unknown user"))
            }
            MutateCollectionError::User(ref e @ UserMutationError::DuplicateUsername(_)) => (
                Some("already-exists"),
                AdminError::conflict("username_already_exists", e.to_string()),
            ),
            MutateCollectionError::User(UserMutationError::LastUserProtected) => (
                Some("last-user-protected"),
                AdminError::conflict(
                    "last_user_protected",
                    "cannot delete the last remaining user",
                ),
            ),
            MutateCollectionError::User(
                ref e @ (UserMutationError::InvalidUsername | UserMutationError::BadHash),
            ) => (None, AdminError::bad_request("invalid_user", e.to_string())),
            MutateCollectionError::Io(e) => (
                Some("io-failure"),
                AdminError::internal("persisting user collection", e),
            ),
        }
    }
}

/// Read-modify-write on the user collection, serialised across
/// concurrent admin requests via `user_collection_lock`. The closure
/// returns whatever value the handler needs back, plus a
/// [`CollectionWrite`] saying which record changed; `mutate_collection`
/// performs the corresponding `save_user` or `delete_user` after the
/// closure succeeds. On any error nothing is persisted.
pub(crate) fn mutate_collection<T, F>(
    state: &ListenerState,
    f: F,
) -> Result<T, MutateCollectionError>
where
    F: FnOnce(&mut crate::users::UserCollection) -> Result<(T, CollectionWrite), UserMutationError>,
{
    let _guard = state.admin().user_collection_lock.lock().unwrap();
    let mut collection =
        crate::users::load(&state.admin().users_dir).map_err(MutateCollectionError::Io)?;
    let (out, write) = f(&mut collection).map_err(MutateCollectionError::User)?;
    match write {
        CollectionWrite::Save(id) => {
            let record = collection
                .find_by_id(&id)
                .expect("Save(id) requires the record to be present in the collection");
            crate::users::save_user(&state.admin().users_dir, record)
                .map_err(MutateCollectionError::Io)?;
        }
        CollectionWrite::Delete(id) => {
            crate::users::delete_user(&state.admin().users_dir, &id)
                .map_err(MutateCollectionError::Io)?;
        }
    }
    Ok(out)
}

/// Audit shim for the user-CRUD handlers. On `Ok(_)` emits a success
/// event; on `Err(_)` classifies the error into an `AdminError` +
/// audit reason and emits a failure event. The success and failure
/// usernames come from the caller — handlers know both the path id
/// they were given and the up-to-date username.
fn audit_user_mutation<T>(
    action: &str,
    principal: &Principal,
    username: &str,
    result: Result<T, MutateCollectionError>,
) -> Result<T, AdminError> {
    match result {
        Ok(value) => {
            log_admin_event(
                action,
                principal,
                AuditEntity::User { username },
                None,
                "success",
            );
            Ok(value)
        }
        Err(e) => {
            let (reason, response) = e.into_admin_response();
            log_admin_event(
                action,
                principal,
                AuditEntity::User { username },
                reason,
                "failure",
            );
            Err(response)
        }
    }
}

pub(crate) fn openapi_router() -> OpenApiRouter<Arc<ListenerState>> {
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
    State(state): State<Arc<ListenerState>>,
) -> Result<Json<AdminUsersListResponse>, AdminError> {
    let collection = crate::users::load(&state.admin().users_dir).map_err(|e| {
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
    security(("session_cookie" = [], "csrf_header" = [])),
    request_body = AdminUserCreateRequest,
    responses(
        (status = 200, description = "Operator created.", body = AdminUserListItem),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 409, description = "Username already exists.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_user_create(
    State(state): State<Arc<ListenerState>>,
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
    let now = crate::time::now_epoch_secs();
    let record = crate::users::UserRecord {
        id: crate::users::new_user_id(),
        username: username_norm.clone(),
        password_hash,
        created_at: now,
        password_changed_at: now,
    };
    let target_username = record.username.clone();
    let saved = audit_user_mutation(
        "user.create",
        &principal,
        &target_username,
        mutate_collection(&state, |c| {
            let id = record.id.clone();
            c.add(record.clone())?;
            Ok((record.clone(), CollectionWrite::Save(id)))
        }),
    )?;
    Ok(Json(user_list_item(&saved)))
}

#[utoipa::path(
    put,
    path = "/v1/admin/users/{id}",
    tag = "users",
    security(("session_cookie" = [], "csrf_header" = [])),
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
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
    Body(body): Body<AdminUserUpdateRequest>,
) -> Result<Json<AdminUserListItem>, AdminError> {
    let updated = audit_user_mutation(
        "user.update",
        &principal,
        &id,
        mutate_collection(&state, |c| {
            let updated = c.update(&id, |u| {
                if let Some(username) = &body.username {
                    u.username = username.clone();
                }
            })?;
            Ok((updated.clone(), CollectionWrite::Save(id.clone())))
        }),
    )?;
    Ok(Json(user_list_item(&updated)))
}

#[utoipa::path(
    post,
    path = "/v1/admin/users/{id}/password",
    tag = "users",
    security(("session_cookie" = [], "csrf_header" = [])),
    params(("id" = String, Path, description = "Operator id.")),
    request_body = AdminUserResetPasswordRequest,
    responses(
        (status = 200, description = "Password reset; the target user's sessions are invalidated automatically.", body = AdminOkUserResponse),
        (status = 400, description = "Validation failed.", body = ServiceErrorResponse),
        (status = 404, description = "Operator not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_user_reset_password(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
    Body(body): Body<AdminUserResetPasswordRequest>,
) -> Result<Json<AdminOkUserResponse>, AdminError> {
    crate::admin_api::util::validate_password_or_bad_request(&body.password)?;
    let new_hash = crate::auth::hash_password(&body.password).map_err(|e| {
        warn!(err = %e, "users.reset-password: argon2 hash failed");
        AdminError::internal_with_code("internal", "could not hash password")
    })?;
    let now = crate::time::now_epoch_secs();
    // The target operator's existing sessions are invalidated
    // automatically: `axum_login` records the password-hash bytes in
    // each session and re-checks them on every request via
    // `session_auth_hash()`. Once we write the new hash here, the next
    // request that arrives carrying any pre-rotation session for this
    // user fails the hash check and the session is flushed
    // server-side.
    audit_user_mutation(
        "user.password.reset",
        &principal,
        &id,
        mutate_collection(&state, |c| {
            c.update(&id, |u| {
                u.password_hash = new_hash.clone();
                u.password_changed_at = now;
            })?;
            Ok(((), CollectionWrite::Save(id.clone())))
        }),
    )?;
    Ok(Json(AdminOkUserResponse { ok: true }))
}

#[utoipa::path(
    delete,
    path = "/v1/admin/users/{id}",
    tag = "users",
    security(("session_cookie" = [], "csrf_header" = [])),
    params(("id" = String, Path, description = "Operator id.")),
    responses(
        (status = 200, description = "Operator deleted; all sessions held by the deleted user are invalidated automatically.", body = AdminOkUserResponse),
        (status = 400, description = "Cannot delete the last operator.", body = ServiceErrorResponse),
        (status = 404, description = "Operator not found.", body = ServiceErrorResponse),
    ),
)]
pub(crate) async fn handle_admin_user_delete(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(principal): axum::Extension<Principal>,
    Path(id): Path<String>,
) -> Result<Json<AdminOkUserResponse>, AdminError> {
    // Per-user session revocation is automatic: once the record is
    // gone, `AuthBackend::get_user` returns None on any subsequent
    // request, `auth_session.user` is None, and the
    // `require_login_layer` rejects with 401.
    audit_user_mutation(
        "user.delete",
        &principal,
        &id,
        mutate_collection(&state, |c| {
            let removed = c.remove(&id)?;
            Ok((removed, CollectionWrite::Delete(id.clone())))
        }),
    )?;
    Ok(Json(AdminOkUserResponse { ok: true }))
}

#[cfg(test)]
mod tests;
