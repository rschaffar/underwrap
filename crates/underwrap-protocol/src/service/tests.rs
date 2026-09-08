use super::*;

#[test]
fn runtime_register_request_roundtrips() {
    let req = RuntimeWrapperRegisterRequest {
        wrappers: vec![RuntimeWrapperRegisterPair {
            wrapper_name: "gh".into(),
            resource_name: "github/token".into(),
        }],
    };
    let json = serde_json::to_string(&req).unwrap();
    let parsed: RuntimeWrapperRegisterRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn runtime_deny_reason_wire_strings_are_pinned() {
    // These strings are the wire contract the daemon forwards to wrappers
    // and that integration tests assert on. A rename here is a wire break.
    assert_eq!(RuntimeDenyReason::NoPermission.as_str(), "no-permission");
    assert_eq!(
        RuntimeDenyReason::UnregisteredPair.as_str(),
        "unregistered-pair"
    );
    assert_eq!(RuntimeDenyReason::Locked.as_str(), "locked");
    assert_eq!(RuntimeDenyReason::Expired.as_str(), "expired");
    assert_eq!(
        RuntimeDenyReason::NotUnsafeDebug.as_str(),
        "not-unsafe-debug"
    );
    assert_eq!(RuntimeDenyReason::NotWritable.as_str(), "not-writable");
    // Display matches as_str.
    assert_eq!(RuntimeDenyReason::Expired.to_string(), "expired");
}

#[test]
fn runtime_invalid_blob_reason_wire_strings_are_pinned() {
    assert_eq!(
        RuntimeInvalidBlobReason::DecodeFailed.as_str(),
        "decode-failed"
    );
    assert_eq!(
        RuntimeInvalidBlobReason::SchemaUnknown.as_str(),
        "schema-unknown"
    );
    assert_eq!(
        RuntimeInvalidBlobReason::NoncanonicalBlob.as_str(),
        "noncanonical-blob"
    );
    assert_eq!(
        RuntimeInvalidBlobReason::ResourceTooLarge.as_str(),
        "resource-too-large"
    );
    assert_eq!(
        RuntimeInvalidBlobReason::EnvSectionImmutable.as_str(),
        "env-section-immutable"
    );
}

#[test]
fn rotate_key_response_shape_remains_key_and_warning_only() {
    let value = serde_json::to_value(AdminStoreRotateKeyResponse {
        key_hex: "fixture-key".into(),
        warning: "fixture-warning".into(),
    })
    .unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.len(), 2);
    assert_eq!(object["key_hex"], "fixture-key");
    assert_eq!(object["warning"], "fixture-warning");
}

#[test]
fn error_code_store_locked_is_pinned() {
    assert_eq!(ERROR_CODE_STORE_LOCKED, "store_locked");
}

#[test]
fn admin_store_migration_contract() {
    let key = AdminKeyHex::new("a".repeat(64));
    assert_eq!(format!("{key:?}"), "AdminKeyHex(<redacted>)");
    assert_eq!(
        serde_json::to_value(&key).unwrap(),
        serde_json::json!("a".repeat(64))
    );

    let create = AdminStoreCreateRequest {
        name: "vault".into(),
        backend: AdminStoreBackendKind::Encrypted,
        description: None,
        key_hex: Some(key.clone()),
    };
    assert_eq!(
        serde_json::to_value(create).unwrap(),
        serde_json::json!({
            "name": "vault",
            "backend": "encrypted",
            "key_hex": "a".repeat(64),
        })
    );

    let migrate: AdminStoreMigrateRequest = serde_json::from_value(serde_json::json!({
        "backend": "plain"
    }))
    .unwrap();
    assert_eq!(migrate.backend, AdminStoreBackendKind::Plain);
    assert!(migrate.key_hex.is_none());

    assert_eq!(
        serde_json::to_value(AdminStoreLifecycleState::RecoveryRequired).unwrap(),
        serde_json::json!("recovery-required")
    );
}
