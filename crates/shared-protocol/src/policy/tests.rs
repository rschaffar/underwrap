use super::*;

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

#[test]
fn missing_permission_returns_none() {
    let doc = PolicyDocument::default();
    assert!(doc.evaluate_permission_at("c", "w", "r", 100).is_none());
}

#[test]
fn locked_permission_denies_with_reason() {
    let mut doc = doc_with_permission(WrapperApprovalMode::Locked);
    doc.permissions[0].reason = Some("revoked".into());
    match doc
        .evaluate_permission_at("dev-laptop", "gh-comment", "res_token", 100)
        .unwrap()
    {
        PolicyDecision::Deny { reason } => assert_eq!(reason, "revoked"),
        other => panic!("expected Deny, got {other:?}"),
    }
}

#[test]
fn unlocked_permission_allows() {
    let doc = doc_with_permission(WrapperApprovalMode::Unlocked);
    assert_eq!(
        doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", 100)
            .unwrap(),
        PolicyDecision::Allow,
    );
}

#[test]
fn unlocked_until_permission_allows_then_denies_lazily() {
    let doc = doc_with_permission(WrapperApprovalMode::UnlockedUntil { until_epoch: 200 });
    assert_eq!(
        doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", 199)
            .unwrap(),
        PolicyDecision::Allow,
    );
    assert!(matches!(
        doc.evaluate_permission_at("dev-laptop", "gh-comment", "res_token", 201)
            .unwrap(),
        PolicyDecision::Deny { .. }
    ));
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
        ],
    };
    let json = serde_json::to_string(&doc).unwrap();
    let parsed: PolicyDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.permissions, doc.permissions);
}
