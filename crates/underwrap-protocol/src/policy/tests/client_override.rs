use super::*;

#[test]
fn overrides_preserve_the_mode_variant_and_writable_boundaries() {
    for mode in [
        WrapperApprovalMode::Locked,
        WrapperApprovalMode::Unlocked,
        WrapperApprovalMode::UnlockedUntil { until_epoch: 99 },
        WrapperApprovalMode::UnlockedUntil { until_epoch: 100 },
        WrapperApprovalMode::UnsafeDebugUntil { until_epoch: 99 },
        WrapperApprovalMode::UnsafeDebugUntil { until_epoch: 100 },
    ] {
        for saved_writable in [false, true] {
            let mut permission = doc_with_permission(mode.clone()).permissions.remove(0);
            permission.writable = saved_writable;
            let before = permission.clone();
            for variant in [Real, Debug] {
                let individual = decide(&permission, variant, 100);
                assert_eq!(
                    ClientAccessOverride::Individual {}.decide(Some(&permission), variant, 100),
                    individual,
                );
                assert_eq!(
                    ClientAccessOverride::Individual {}.writable_for(Some(&permission)),
                    saved_writable,
                );
                assert_eq!(
                    ClientAccessOverride::Locked {}.decide(Some(&permission), variant, 100),
                    PolicyDecision::Deny {
                        reason: RuntimeDenyReason::ClientLocked.to_string(),
                    },
                );
                assert!(!ClientAccessOverride::Locked {}.writable_for(Some(&permission)));
                for writable in [false, true] {
                    let access = ClientAccessOverride::Unlocked { writable };
                    assert_eq!(
                        access.decide(Some(&permission), variant, 100),
                        if variant == Real {
                            PolicyDecision::Allow
                        } else {
                            individual.clone()
                        },
                    );
                    assert_eq!(access.writable_for(Some(&permission)), writable);
                }
            }
            assert_eq!(permission, before);
        }
    }
}

#[test]
fn unlock_without_a_row_never_grants_debug() {
    for variant in [Real, Debug] {
        assert!(matches!(
            ClientAccessOverride::Individual {}.decide(None, variant, 100),
            PolicyDecision::Deny { reason } if reason == "no-permission"
        ));
        assert!(matches!(
            ClientAccessOverride::Locked {}.decide(None, variant, 100),
            PolicyDecision::Deny { reason } if reason == "client-locked"
        ));
        for writable in [false, true] {
            let access = ClientAccessOverride::Unlocked { writable };
            assert_eq!(access.writable_for(None), writable);
            assert_eq!(
                access.decide(None, variant, 100),
                if variant == Real {
                    PolicyDecision::Allow
                } else {
                    PolicyDecision::Deny {
                        reason: "no-permission".into(),
                    }
                },
            );
        }
    }
}

#[test]
fn override_format_is_explicit_and_rejects_ambiguous_writable_fields() {
    assert_eq!(
        ClientAccessOverride::default(),
        ClientAccessOverride::Individual {}
    );
    for access in [
        ClientAccessOverride::Individual {},
        ClientAccessOverride::Locked {},
        ClientAccessOverride::Unlocked { writable: false },
        ClientAccessOverride::Unlocked { writable: true },
    ] {
        let value = serde_json::to_value(access).unwrap();
        assert_eq!(
            serde_json::from_value::<ClientAccessOverride>(value).unwrap(),
            access
        );
    }
    for invalid in [
        serde_json::json!({"kind": "unknown"}),
        serde_json::json!({"kind": "unlocked"}),
        serde_json::json!({"kind": "unlocked", "writable": null}),
        serde_json::json!({"kind": "unlocked", "writable": "true"}),
        serde_json::json!({"kind": "individual", "writable": true}),
        serde_json::json!({"kind": "locked", "writable": true}),
    ] {
        assert!(serde_json::from_value::<ClientAccessOverride>(invalid).is_err());
    }
}
