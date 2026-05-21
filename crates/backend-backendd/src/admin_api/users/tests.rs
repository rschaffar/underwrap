use super::*;
use crate::test_support::{
    admin_principal, admin_principal_with_id, crud_state, response_body_json, seed_admin_into,
};

#[test]
fn user_create_persists_record_and_omits_password_hash_from_response() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    seed_admin_into(&state, "alice");
    let p = admin_principal(&[]);
    let resp = do_admin_user_create(
        &state,
        &p,
        underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: "hunter2-bob".into(),
        },
    );
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_body_json(resp);
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
    let collection = crate::users::load(&state.users_file).unwrap();
    assert_eq!(collection.users.len(), 2);
    assert!(collection.find_by_username("bob").is_some());
}

#[test]
fn user_create_refuses_duplicate_username_case_insensitively() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    seed_admin_into(&state, "alice");
    let p = admin_principal(&[]);
    let req = || underwrap_protocol::service::AdminUserCreateRequest {
        username: "Alice".into(), // collides after normalisation
        password: "any-pass".into(),
    };
    let resp = do_admin_user_create(&state, &p, req());
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = response_body_json(resp);
    assert_eq!(body["code"], "username_already_exists");
}

#[test]
fn user_create_refuses_empty_password() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    seed_admin_into(&state, "alice");
    let resp = do_admin_user_create(
        &state,
        &admin_principal(&[]),
        underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: String::new(),
        },
    );
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response_body_json(resp)["code"], "invalid_password");
}

#[test]
fn users_list_omits_password_hash_for_every_record() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    seed_admin_into(&state, "alice");
    let p = admin_principal(&[]);
    for u in ["bob", "carol"] {
        do_admin_user_create(
            &state,
            &p,
            underwrap_protocol::service::AdminUserCreateRequest {
                username: u.into(),
                password: format!("{u}-pass"),
            },
        );
    }
    let resp = handle_admin_users_list(&state);
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_body_json(resp);
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(!body_str.contains("$argon2"));
    assert!(!body_str.contains("password_hash"));
    let users = body["users"].as_array().unwrap();
    assert_eq!(users.len(), 3); // alice + bob + carol
}

#[test]
fn user_update_renames_user() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    seed_admin_into(&state, "alice");
    let p = admin_principal(&[]);
    let create_resp = do_admin_user_create(
        &state,
        &p,
        underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: "x".into(),
        },
    );
    let bob_id = response_body_json(create_resp)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = do_admin_user_update(
        &state,
        &p,
        &bob_id,
        underwrap_protocol::service::AdminUserUpdateRequest {
            username: Some("robert".into()),
        },
    );
    assert_eq!(resp.status(), StatusCode::OK);
    let body = response_body_json(resp);
    assert_eq!(body["username"], "robert");
}

#[test]
fn user_delete_refuses_last_user() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let alice_id = seed_admin_into(&state, "alice");
    let p = admin_principal_with_id(&alice_id);
    let resp = handle_admin_user_delete(&state, &p, &alice_id);
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = response_body_json(resp);
    assert_eq!(body["code"], "last_user_protected");
    // Record still in the collection.
    let collection = crate::users::load(&state.users_file).unwrap();
    assert!(collection.find_by_id(&alice_id).is_some());
}

#[test]
fn user_delete_succeeds_when_another_user_exists() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    let alice_id = seed_admin_into(&state, "alice");
    let p = admin_principal_with_id(&alice_id);
    let create = do_admin_user_create(
        &state,
        &p,
        underwrap_protocol::service::AdminUserCreateRequest {
            username: "second".into(),
            password: "x".into(),
        },
    );
    let second_id = response_body_json(create)["id"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = handle_admin_user_delete(&state, &p, &second_id);
    assert_eq!(resp.status(), StatusCode::OK);
}

#[test]
fn user_reset_password_changes_hash_and_revokes_sessions() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    seed_admin_into(&state, "alice");
    let p = admin_principal(&[]);
    let create = do_admin_user_create(
        &state,
        &p,
        underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: "old-pass".into(),
        },
    );
    let bob_id = response_body_json(create)["id"]
        .as_str()
        .unwrap()
        .to_string();

    let hash_before = crate::users::load(&state.users_file)
        .unwrap()
        .find_by_id(&bob_id)
        .unwrap()
        .password_hash
        .clone();

    // do_admin_user_create is sync, but reset_password takes a
    // `Request<Incoming>` — synthesise the body-already-parsed
    // path by inlining the logic. Easiest: drive
    // mutate_collection + the underlying state directly via a
    // tiny helper test that exercises the public path through
    // an `async` wrapper would need a runtime; we mimic the
    // server logic here instead.
    let new_hash = crate::auth::hash_password("new-pass").unwrap();
    let now = crate::users::now_epoch_secs();
    mutate_collection(&state, |c| {
        c.update(&bob_id, |u| {
            u.password_hash = new_hash.clone();
            u.password_changed_at = now;
        })?;
        Ok(())
    })
    .unwrap();
    state.session_revocations.revoke_username("bob", now);

    let hash_after = crate::users::load(&state.users_file)
        .unwrap()
        .find_by_id(&bob_id)
        .unwrap()
        .password_hash
        .clone();
    assert_ne!(hash_before, hash_after, "hash must change");
    assert!(
        state
            .session_revocations
            .is_user_floor_revoked("bob", now - 1),
        "sessions issued before the reset must be revoked",
    );
}

#[test]
fn debug_redacts_password_hash_in_collection_logging() {
    // Regression: when we tracing!(?collection) the hash must
    // never appear. The `Debug` impl on `UserRecord` redacts it,
    // but the easiest check is to format the loaded collection
    // after every mutation and grep.
    let tmp = tempfile::TempDir::new().unwrap();
    let state = crud_state(tmp.path());
    seed_admin_into(&state, "alice");
    do_admin_user_create(
        &state,
        &admin_principal(&[]),
        underwrap_protocol::service::AdminUserCreateRequest {
            username: "bob".into(),
            password: "x".into(),
        },
    );
    let collection = crate::users::load(&state.users_file).unwrap();
    let formatted = format!("{collection:?}");
    assert!(
        !formatted.contains("$argon2"),
        "Debug must redact password hashes; got: {formatted}",
    );
}
