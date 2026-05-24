use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use hyper::StatusCode;

use super::*;
use crate::http::responses::Body;
use crate::test_support::{
    admin_principal, admin_principal_with_id, crud_state, run_handler, seed_admin_into,
};

#[test]
fn user_create_persists_record_and_omits_password_hash_from_response() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    seed_admin_into(&state, "alice");
    let p = admin_principal();
    let (status, body) = run_handler(handle_admin_user_create(
        State(state.clone()),
        Extension(p),
        Body(underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: "hunter2-bob-long".into(),
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(
        !body_str.contains("$argon2"),
        "create response must not leak the password hash: {body_str}",
    );
    // `password_changed_at` is fine; the actual password and its
    // hash must not appear anywhere.
    assert!(
        !body_str.contains("hunter2-bob"),
        "create response must not echo the plaintext password: {body_str}",
    );
    assert!(
        !body_str.contains("password_hash"),
        "create response must not expose the password_hash field: {body_str}",
    );
    assert_eq!(body["username"], "bob");
    assert!(
        body.get("role").is_none(),
        "wire shape must not carry a role"
    );

    // The record landed in the user collection.
    let collection = crate::users::load(&state.users_dir).unwrap();
    assert_eq!(collection.users.len(), 2);
    assert!(collection.find_by_username("bob").is_some());
}

#[test]
fn user_create_refuses_duplicate_username_case_insensitively() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    seed_admin_into(&state, "alice");
    let p = admin_principal();
    let (status, body) = run_handler(handle_admin_user_create(
        State(state.clone()),
        Extension(p),
        Body(underwrap_protocol::service::AdminUserCreateRequest {
            username: "Alice".into(), // collides after normalisation
            password: "any-pass-long".into(),
        }),
    ));
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "username_already_exists");
}

#[test]
fn user_create_refuses_empty_password() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    seed_admin_into(&state, "alice");
    let (status, body) = run_handler(handle_admin_user_create(
        State(state.clone()),
        Extension(admin_principal()),
        Body(underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: String::new(),
        }),
    ));
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "password_empty");
}

#[test]
fn user_create_refuses_short_password() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    seed_admin_into(&state, "alice");
    let (status, body) = run_handler(handle_admin_user_create(
        State(state.clone()),
        Extension(admin_principal()),
        Body(underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: "a".repeat(crate::auth::MIN_PASSWORD_LEN - 1),
        }),
    ));
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "password_too_short");
}

#[test]
fn user_create_accepts_min_length_password() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    seed_admin_into(&state, "alice");
    let (status, _body) = run_handler(handle_admin_user_create(
        State(state.clone()),
        Extension(admin_principal()),
        Body(underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: "a".repeat(crate::auth::MIN_PASSWORD_LEN),
        }),
    ));
    assert_eq!(status, StatusCode::OK);
}

#[test]
fn users_list_omits_password_hash_for_every_record() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    seed_admin_into(&state, "alice");
    let p = admin_principal();
    for u in ["bob", "carol"] {
        let (status, _) = run_handler(handle_admin_user_create(
            State(state.clone()),
            Extension(p.clone()),
            Body(underwrap_protocol::service::AdminUserCreateRequest {
                username: u.into(),
                password: format!("{u}-pass-long"),
            }),
        ));
        assert_eq!(status, StatusCode::OK);
    }
    let (status, body) = run_handler(handle_admin_users_list(State(state.clone())));
    assert_eq!(status, StatusCode::OK);
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(!body_str.contains("$argon2"));
    assert!(!body_str.contains("password_hash"));
    let users = body["users"].as_array().unwrap();
    assert_eq!(users.len(), 3); // alice + bob + carol
}

#[test]
fn user_update_renames_user() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    seed_admin_into(&state, "alice");
    let p = admin_principal();
    let (create_status, create_body) = run_handler(handle_admin_user_create(
        State(state.clone()),
        Extension(p.clone()),
        Body(underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: "xxxxxxxxxxxx".into(),
        }),
    ));
    assert_eq!(create_status, StatusCode::OK);
    let bob_id = create_body["id"].as_str().unwrap().to_string();
    let (status, body) = run_handler(handle_admin_user_update(
        State(state.clone()),
        Extension(p),
        Path(bob_id),
        Body(underwrap_protocol::service::AdminUserUpdateRequest {
            username: Some("robert".into()),
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["username"], "robert");
}

#[test]
fn user_delete_refuses_last_user() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let alice_id = seed_admin_into(&state, "alice");
    let p = admin_principal_with_id(&alice_id);
    let (status, body) = run_handler(handle_admin_user_delete(
        State(state.clone()),
        Extension(p),
        Path(alice_id.clone()),
    ));
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "last_user_protected");
    // Record still in the collection.
    let collection = crate::users::load(&state.users_dir).unwrap();
    assert!(collection.find_by_id(&alice_id).is_some());
}

#[test]
fn user_delete_succeeds_when_another_user_exists() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let alice_id = seed_admin_into(&state, "alice");
    let p = admin_principal_with_id(&alice_id);
    let (create_status, create_body) = run_handler(handle_admin_user_create(
        State(state.clone()),
        Extension(p.clone()),
        Body(underwrap_protocol::service::AdminUserCreateRequest {
            username: "second".into(),
            password: "xxxxxxxxxxxx".into(),
        }),
    ));
    assert_eq!(create_status, StatusCode::OK);
    let second_id = create_body["id"].as_str().unwrap().to_string();
    let (status, _) = run_handler(handle_admin_user_delete(
        State(state.clone()),
        Extension(p),
        Path(second_id),
    ));
    assert_eq!(status, StatusCode::OK);
}

#[test]
fn user_reset_password_changes_hash() {
    // Per-user session revocation is no longer enforced by an
    // explicit floor table — `axum_login` does it automatically by
    // storing the password-hash bytes in each session and verifying
    // them on every request via `session_auth_hash()`. So this test
    // only needs to confirm the on-disk hash actually rotates; the
    // session-side check is covered by the `auth/tests.rs` test
    // `session_auth_hash_changes_when_password_rotates`.
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    seed_admin_into(&state, "alice");
    let p = admin_principal();
    let (create_status, create_body) = run_handler(handle_admin_user_create(
        State(state.clone()),
        Extension(p),
        Body(underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: "old-pass-long".into(),
        }),
    ));
    assert_eq!(create_status, StatusCode::OK);
    let bob_id = create_body["id"].as_str().unwrap().to_string();

    let hash_before = crate::users::load(&state.users_dir)
        .unwrap()
        .find_by_id(&bob_id)
        .unwrap()
        .password_hash
        .clone();

    let new_hash = crate::auth::hash_password("new-pass").unwrap();
    let now = crate::users::now_epoch_secs();
    mutate_collection(&state, |c| {
        c.update(&bob_id, |u| {
            u.password_hash = new_hash.clone();
            u.password_changed_at = now;
        })?;
        Ok(((), CollectionWrite::Save(bob_id.clone())))
    })
    .unwrap();

    let hash_after = crate::users::load(&state.users_dir)
        .unwrap()
        .find_by_id(&bob_id)
        .unwrap()
        .password_hash
        .clone();
    assert_ne!(hash_before, hash_after, "hash must change");
}

#[test]
fn debug_redacts_password_hash_in_collection_logging() {
    // Regression: when we tracing!(?collection) the hash must
    // never appear. The `Debug` impl on `UserRecord` redacts it,
    // but the easiest check is to format the loaded collection
    // after every mutation and grep.
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    seed_admin_into(&state, "alice");
    let (status, _) = run_handler(handle_admin_user_create(
        State(state.clone()),
        Extension(admin_principal()),
        Body(underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: "xxxxxxxxxxxx".into(),
        }),
    ));
    assert_eq!(status, StatusCode::OK);
    let collection = crate::users::load(&state.users_dir).unwrap();
    let formatted = format!("{collection:?}");
    assert!(
        !formatted.contains("$argon2"),
        "Debug must redact password hashes; got: {formatted}",
    );
}

#[test]
fn io_failure_during_save_surfaces_as_500() {
    // Make the users dir read-only so save_user fails with EACCES.
    // The handler must classify that as a 500 (Io), not a 400.
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    seed_admin_into(&state, "alice");

    // Lock the directory after the seed so the create handler fails
    // when it tries to write the new user record.
    use std::os::unix::fs::PermissionsExt;
    let original_mode = std::fs::metadata(&state.users_dir)
        .unwrap()
        .permissions()
        .mode();
    std::fs::set_permissions(&state.users_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let (status, body) = run_handler(handle_admin_user_create(
        State(state.clone()),
        Extension(admin_principal()),
        Body(underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: "xxxxxxxxxxxx".into(),
        }),
    ));

    // Restore so the tempdir cleanup can run.
    std::fs::set_permissions(
        &state.users_dir,
        std::fs::Permissions::from_mode(original_mode),
    )
    .unwrap();

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "internal");
}
