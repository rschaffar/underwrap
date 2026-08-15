//! Shape coverage for [`super::log_admin_event`]: one assertion per
//! [`AuditEntity`] arm verifying the formatted `resource` field and
//! whether `wrapper` is surfaced. Operators grep these prefixes, so
//! the shape is part of the audit contract.

use underwrap_protocol::policy::{WrapperApprovalMode, WrapperPermission};
use underwrap_protocol::resource::ResourceSlot;

use super::*;
use crate::admin_api::csrf::Principal;
use crate::test_support::capture_tracing_output;

fn capture(run: impl FnOnce()) -> String {
    capture_tracing_output(run).1
}

fn principal() -> Principal {
    Principal {
        id: "u-test".into(),
        username: "alice".into(),
    }
}

fn permission() -> WrapperPermission {
    WrapperPermission {
        client_instance: "inst-1".into(),
        wrapper: "gh-authorized".into(),
        resource_id: "github/token".into(),
        mode: WrapperApprovalMode::Unlocked,
        writable: false,
        reason: None,
    }
}

fn assert_contains_all(haystack: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            haystack.contains(needle),
            "expected log to contain {needle:?}, got: {haystack}",
        );
    }
}

#[test]
fn log_admin_event_user_variant_formats_resource_and_omits_wrapper() {
    let out = capture(|| {
        log_admin_event(
            "user.create",
            &principal(),
            AuditEntity::User { username: "bob" },
            None,
            "success",
        );
    });
    assert_contains_all(
        &out,
        &[
            "admin event",
            "action=user.create",
            "operator=alice",
            "wrapper=None",
            "resource=Some(\"user:bob\")",
            "result=success",
        ],
    );
}

#[test]
fn log_admin_event_store_variant_formats_resource_as_store_prefix() {
    let out = capture(|| {
        log_admin_event(
            "store.create",
            &principal(),
            AuditEntity::Store { store: "vault" },
            None,
            "success",
        );
    });
    assert_contains_all(&out, &["resource=Some(\"store:vault\")", "wrapper=None"]);
}

#[test]
fn log_admin_event_client_instance_variant_formats_resource_as_client_instance_prefix() {
    let out = capture(|| {
        log_admin_event(
            "client_instance.create",
            &principal(),
            AuditEntity::ClientInstance { id: "inst-42" },
            None,
            "success",
        );
    });
    assert_contains_all(
        &out,
        &["resource=Some(\"client-instance:inst-42\")", "wrapper=None"],
    );
}

#[test]
fn log_admin_event_resource_variant_formats_resource_as_resource_prefix() {
    let out = capture(|| {
        log_admin_event(
            "resource.create",
            &principal(),
            AuditEntity::Resource {
                resource_id: "res_abc",
            },
            None,
            "success",
        );
    });
    assert_contains_all(
        &out,
        &["resource=Some(\"resource:res_abc\")", "wrapper=None"],
    );
}

#[test]
fn log_admin_event_pair_variant_surfaces_both_wrapper_and_resource() {
    let out = capture(|| {
        log_admin_event(
            "pair.create",
            &principal(),
            AuditEntity::Pair {
                wrapper: "gh-authorized",
                resource_id: "github/token",
            },
            None,
            "success",
        );
    });
    assert_contains_all(
        &out,
        &[
            "wrapper=Some(\"gh-authorized\")",
            "resource=Some(\"pair:gh-authorized/github/token\")",
        ],
    );
}

#[test]
fn log_admin_event_permission_variant_carries_full_tuple_in_resource_field() {
    let perm = permission();
    let out = capture(|| {
        log_admin_event(
            "wrapper_permission.upsert",
            &principal(),
            AuditEntity::Permission {
                store: "vault",
                permission: &perm,
            },
            None,
            "success",
        );
    });
    assert_contains_all(
        &out,
        &[
            "wrapper=Some(\"gh-authorized\")",
            "resource=Some(\"wrapper_permission:vault/inst-1/gh-authorized/github/token\")",
        ],
    );
}

#[test]
fn log_admin_event_secret_variant_encodes_store_resource_slot_key() {
    let out = capture(|| {
        log_admin_event(
            "secret.write",
            &principal(),
            AuditEntity::Secret {
                store: "vault",
                resource_id: "github/token",
                slot: ResourceSlot::Env,
                key: "GH_TOKEN",
            },
            None,
            "success",
        );
    });
    assert_contains_all(
        &out,
        &[
            "resource=Some(\"secret:vault/github/token/env/GH_TOKEN\")",
            "wrapper=None",
        ],
    );
}

#[test]
fn log_admin_event_propagates_operation_and_result_strings() {
    let out = capture(|| {
        log_admin_event(
            "user.create",
            &principal(),
            AuditEntity::User { username: "bob" },
            Some("invalid-password"),
            "failure",
        );
    });
    assert_contains_all(
        &out,
        &["operation=Some(\"invalid-password\")", "result=failure"],
    );
}
