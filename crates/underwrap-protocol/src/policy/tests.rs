use super::*;

mod client_override;

fn doc_with_permission(mode: WrapperApprovalMode) -> PolicyDocument {
    PolicyDocument {
        permissions: vec![WrapperPermission {
            client_instance: "dev-laptop".to_string(),
            wrapper: "gh-comment".to_string(),
            resource_id: "res_token".to_string(),
            mode,
            writable: false,
            reason: None,
        }],
    }
}

use crate::variant::WrapperVariant::{Debug, Real};

#[test]
fn missing_permission_returns_none() {
    let doc = PolicyDocument::default();
    assert!(
        doc.evaluate_permission_at("c", "w", "r", Real, 100)
            .is_none()
    );
}

#[test]
fn locked_permission_denies_with_reason() {
    let mut doc = doc_with_permission(WrapperApprovalMode::Locked);
    doc.permissions[0].reason = Some("revoked".into());
    for variant in [Real, Debug] {
        match doc
            .evaluate_permission_at("dev-laptop", "gh-comment", "res_token", variant, 100)
            .unwrap()
        {
            PolicyDecision::Deny { reason } => assert_eq!(reason, "revoked"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }
}

#[test]
fn unlocked_permission_allows_real_but_denies_debug() {
    let doc = doc_with_permission(WrapperApprovalMode::Unlocked);
    assert_eq!(
        doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", Real, 100)
            .unwrap(),
        PolicyDecision::Allow,
    );
    assert!(matches!(
        doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", Debug, 100)
            .unwrap(),
        PolicyDecision::Deny { .. }
    ));
}

#[test]
fn unlocked_until_permission_allows_real_then_denies_lazily_and_never_debug() {
    let doc = doc_with_permission(WrapperApprovalMode::UnlockedUntil { until_epoch: 200 });
    assert_eq!(
        doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", Real, 199)
            .unwrap(),
        PolicyDecision::Allow,
    );
    // Debug denied even before expiry (wrong mode).
    assert!(matches!(
        doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", Debug, 199)
            .unwrap(),
        PolicyDecision::Deny { .. }
    ));
    // Real denied after expiry.
    assert!(matches!(
        doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", Real, 201)
            .unwrap(),
        PolicyDecision::Deny { .. }
    ));
}

#[test]
fn unsafe_debug_until_allows_both_variants_then_denies_after_expiry() {
    let doc = doc_with_permission(WrapperApprovalMode::UnsafeDebugUntil { until_epoch: 200 });
    for variant in [Real, Debug] {
        assert_eq!(
            doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", variant, 199)
                .unwrap(),
            PolicyDecision::Allow,
            "variant {variant:?} should be allowed before expiry",
        );
        assert!(
            matches!(
                doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", variant, 201)
                    .unwrap(),
                PolicyDecision::Deny { .. }
            ),
            "variant {variant:?} should be denied after expiry",
        );
    }
}

#[test]
fn writable_defaults_to_false_without_permission() {
    let doc = PolicyDocument::default();
    assert!(!doc.writable_for("dev-laptop", "gh", "r"));
}

#[test]
fn writable_honours_per_permission_flag() {
    let mut doc = doc_with_permission(WrapperApprovalMode::Unlocked);
    assert!(!doc.writable_for("dev-laptop", "gh-comment", "res_token"));
    doc.permissions[0].writable = true;
    assert!(doc.writable_for("dev-laptop", "gh-comment", "res_token"));
}

#[test]
fn document_roundtrips_through_json_with_each_mode() {
    let doc = PolicyDocument {
        permissions: vec![
            WrapperPermission {
                client_instance: "a".into(),
                wrapper: "w1".into(),
                resource_id: "r1".into(),
                mode: WrapperApprovalMode::Locked,
                writable: false,
                reason: Some("r".into()),
            },
            WrapperPermission {
                client_instance: "a".into(),
                wrapper: "w2".into(),
                resource_id: "r2".into(),
                mode: WrapperApprovalMode::UnlockedUntil { until_epoch: 42 },
                writable: true,
                reason: None,
            },
            WrapperPermission {
                client_instance: "a".into(),
                wrapper: "w3".into(),
                resource_id: "r3".into(),
                mode: WrapperApprovalMode::UnsafeDebugUntil { until_epoch: 99 },
                writable: false,
                reason: None,
            },
        ],
    };
    let json = serde_json::to_string(&doc).unwrap();
    // The new mode serialises to its kebab-case wire name.
    assert!(
        json.contains("\"kind\":\"unsafe-debug-until\""),
        "json: {json}"
    );
    let parsed: PolicyDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.permissions, doc.permissions);
}
