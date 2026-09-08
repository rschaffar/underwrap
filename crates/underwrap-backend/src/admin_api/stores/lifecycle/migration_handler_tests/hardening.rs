use std::io;
use std::sync::{Arc, Condvar, Mutex};

use axum::extract::{FromRequest, Request};

use super::*;

struct BlockingCleanupHooks {
    cleanup_entered: tokio::sync::Notify,
    cleanup_gate: (Mutex<bool>, Condvar),
}

impl BlockingCleanupHooks {
    fn new() -> Self {
        Self {
            cleanup_entered: tokio::sync::Notify::new(),
            cleanup_gate: (Mutex::new(false), Condvar::new()),
        }
    }

    fn release_cleanup(&self) {
        *self.cleanup_gate.0.lock().unwrap() = true;
        self.cleanup_gate.1.notify_all();
    }
}

impl migration::MigrationHooks for BlockingCleanupHooks {
    fn migration_id(&self) -> String {
        "1234567890abcdef".into()
    }

    fn checkpoint(&self, phase: migration::MigrationPhase) -> io::Result<()> {
        if phase != migration::MigrationPhase::Cleanup {
            return Ok(());
        }
        self.cleanup_entered.notify_one();
        let mut released = self.cleanup_gate.0.lock().unwrap();
        while !*released {
            released = self.cleanup_gate.1.wait(released).unwrap();
        }
        Err(io::Error::other("injected cleanup failure"))
    }
}

struct BeforeLifecycleLockHooks {
    reached: tokio::sync::Notify,
}

impl migration::MigrationHooks for BeforeLifecycleLockHooks {
    fn migration_id(&self) -> String {
        "fedcba0987654321".into()
    }

    fn before_lifecycle_lock(&self) {
        self.reached.notify_one();
    }
}

struct BlockingReadHooks {
    entered: tokio::sync::Notify,
    gate: (Mutex<bool>, Condvar),
}

impl BlockingReadHooks {
    fn new() -> Self {
        Self {
            entered: tokio::sync::Notify::new(),
            gate: (Mutex::new(false), Condvar::new()),
        }
    }

    fn release(&self) {
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }
}

impl migration::MigrationHooks for BlockingReadHooks {
    fn migration_id(&self) -> String {
        "0123456789abcdef".into()
    }

    fn checkpoint(&self, phase: migration::MigrationPhase) -> io::Result<()> {
        if phase != migration::MigrationPhase::ReadSource {
            return Ok(());
        }
        self.entered.notify_one();
        let mut released = self.gate.0.lock().unwrap();
        while !*released {
            released = self.gate.1.wait(released).unwrap();
        }
        Ok(())
    }
}

#[test]
fn raw_json_omission_succeeds_but_explicit_null_is_invalid_before_mutation() {
    let tmp = tempfile::TempDir::new().unwrap();
    let before = tree_snapshot(tmp.path());

    let create_request = Request::builder()
        .body(axum::body::Body::from(
            r#"{"name":"plain","backend":"plain"}"#,
        ))
        .unwrap();
    let Body(create) = block_on(Body::<AdminStoreCreateRequest>::from_request(
        create_request,
        &(),
    ))
    .unwrap_or_else(|_| panic!("omitted create key must deserialize"));
    assert!(create.key_hex.is_none());

    let migrate_request = Request::builder()
        .body(axum::body::Body::from(r#"{"backend":"plain"}"#))
        .unwrap();
    let Body(migrate_body) = block_on(Body::<AdminStoreMigrateRequest>::from_request(
        migrate_request,
        &(),
    ))
    .unwrap_or_else(|_| panic!("omitted migration key must deserialize"));
    assert!(migrate_body.key_hex.is_none());

    for json in [
        r#"{"name":"plain","backend":"plain","key_hex":null}"#,
        r#"{"name":"vault","backend":"encrypted","key_hex":null}"#,
    ] {
        let request = Request::builder()
            .body(axum::body::Body::from(json))
            .unwrap();
        let rejection = match block_on(Body::<AdminStoreCreateRequest>::from_request(request, &()))
        {
            Ok(_) => panic!("explicit create key null must be rejected"),
            Err(rejection) => rejection,
        };
        assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_body_json(rejection)["code"], "invalid_request");
    }

    for json in [
        r#"{"backend":"plain","key_hex":null}"#,
        r#"{"backend":"encrypted","key_hex":null}"#,
    ] {
        let request = Request::builder()
            .body(axum::body::Body::from(json))
            .unwrap();
        let rejection = match block_on(Body::<AdminStoreMigrateRequest>::from_request(request, &()))
        {
            Ok(_) => panic!("explicit migration key null must be rejected"),
            Err(rejection) => rejection,
        };
        assert_eq!(rejection.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_body_json(rejection)["code"], "invalid_request");
    }

    assert_eq!(tree_snapshot(tmp.path()), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn slow_post_drain_migration_keeps_tokio_responsive_and_publishes_after_abort() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let store_id = create_plain(state.clone(), "work");
    state
        .shared
        .stores
        .read()
        .unwrap()
        .expect_unlocked(&store_id)
        .write("resource/item", b"value")
        .unwrap();
    let hooks = Arc::new(BlockingReadHooks::new());
    let entered = hooks.entered.notified();
    let task = tokio::spawn({
        let state = Arc::clone(&state);
        let store_id = store_id.clone();
        let hooks = Arc::clone(&hooks);
        async move {
            migrate_with_hooks(
                state,
                admin_principal(),
                store_id,
                AdminStoreMigrateRequest {
                    backend: AdminStoreBackendKind::Encrypted,
                    key_hex: Some(AdminKeyHex::new("ab".repeat(32))),
                },
                hooks.as_ref(),
            )
            .await
        }
    });
    entered.await;

    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        tokio::task::yield_now().await;
    })
    .await
    .expect("the single Tokio worker must remain responsive");

    task.abort();
    hooks.release();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let published = {
                let registry = state.shared.stores.read().unwrap();
                let entry = registry.entry(&store_id).unwrap();
                crate::admin_api::stores::backend_kind(&entry.record.storage)
                    == AdminStoreBackendKind::Encrypted
                    && matches!(entry.state, StoreState::Unlocked(_))
            };
            if published {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the non-cancellable post-drain section must publish after abort");
    let _ = task.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recovery_required_precedes_same_backend_initially_and_after_locked_revalidation() {
    let tmp = tempfile::TempDir::new().unwrap();
    let state = Arc::new(crud_state(tmp.path()));
    let store_id = create_plain(state.clone(), "work");
    let first_hooks = Arc::new(BlockingCleanupHooks::new());
    let cleanup_entered = first_hooks.cleanup_entered.notified();
    let first = tokio::spawn({
        let state = Arc::clone(&state);
        let store_id = store_id.clone();
        let hooks = Arc::clone(&first_hooks);
        async move {
            migrate_with_hooks(
                state,
                admin_principal(),
                store_id,
                AdminStoreMigrateRequest {
                    backend: AdminStoreBackendKind::Encrypted,
                    key_hex: Some(AdminKeyHex::new("ab".repeat(32))),
                },
                hooks.as_ref(),
            )
            .await
        }
    });
    cleanup_entered.await;

    let second_hooks = Arc::new(BeforeLifecycleLockHooks {
        reached: tokio::sync::Notify::new(),
    });
    let second_reached_lock = second_hooks.reached.notified();
    let second = tokio::spawn({
        let state = Arc::clone(&state);
        let store_id = store_id.clone();
        let hooks = Arc::clone(&second_hooks);
        async move {
            migrate_with_hooks(
                state,
                admin_principal(),
                store_id,
                AdminStoreMigrateRequest {
                    backend: AdminStoreBackendKind::Encrypted,
                    key_hex: Some(AdminKeyHex::new("cd".repeat(32))),
                },
                hooks.as_ref(),
            )
            .await
        }
    });
    second_reached_lock.await;
    first_hooks.release_cleanup();

    for result in [first.await.unwrap(), second.await.unwrap()] {
        let response = result.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            response_body_json(response)["code"],
            "migration_recovery_required"
        );
    }

    {
        let registry = state.shared.stores.read().unwrap();
        let entry = registry.entry(&store_id).unwrap();
        assert_eq!(
            crate::admin_api::stores::backend_kind(&entry.record.storage),
            AdminStoreBackendKind::Encrypted
        );
        assert!(matches!(entry.state, StoreState::RecoveryRequired));
    }
    let before_retry = tree_snapshot(tmp.path());
    let (status, body) = migrate(
        Arc::clone(&state),
        &store_id,
        AdminStoreBackendKind::Encrypted,
        Some(AdminKeyHex::new("ef".repeat(32))),
    );
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["code"], "migration_recovery_required");
    assert_eq!(tree_snapshot(tmp.path()), before_retry);
}
