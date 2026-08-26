use super::*;
use crate::test_support::{
    TestStore, make_runtime_api_fixture, response_body_json, typed_response_body,
};

// ===== Unit tests for the per-stage helpers =====

#[test]
fn validate_register_request_rejects_empty_names() {
    let req = RuntimeWrapperRegisterRequest {
        wrappers: vec![RuntimeWrapperRegisterPair {
            wrapper_name: "  ".into(),
            resource_name: "ok".into(),
        }],
    };
    use axum::response::IntoResponse;
    let err = validate_register_request(&req).expect_err("empty wrapper_name");
    assert_eq!(err.into_response().status(), 400);

    let req = RuntimeWrapperRegisterRequest {
        wrappers: vec![RuntimeWrapperRegisterPair {
            wrapper_name: "ok".into(),
            resource_name: "  ".into(),
        }],
    };
    let err = validate_register_request(&req).expect_err("empty resource_name");
    assert_eq!(err.into_response().status(), 400);
}

#[test]
fn validate_register_request_accepts_nonempty() {
    let req = RuntimeWrapperRegisterRequest {
        wrappers: vec![RuntimeWrapperRegisterPair {
            wrapper_name: "w".into(),
            resource_name: "r".into(),
        }],
    };
    validate_register_request(&req).unwrap();
}

#[test]
fn validate_register_request_rejects_exact_duplicate_pairs() {
    let req = RuntimeWrapperRegisterRequest {
        wrappers: vec![
            RuntimeWrapperRegisterPair {
                wrapper_name: "wrapper".into(),
                resource_name: "resource".into(),
            },
            RuntimeWrapperRegisterPair {
                wrapper_name: "wrapper".into(),
                resource_name: "resource".into(),
            },
        ],
    };

    let body = response_body_json(validate_register_request(&req).unwrap_err());
    assert_eq!(
        body,
        serde_json::json!({
            "__status": 400,
            "code": "duplicate_wrapper_resource_pair",
            "message": "request contains a duplicate wrapper/resource pair",
        })
    );
}

#[test]
fn validate_register_request_checks_all_names_before_duplicates() {
    for (invalid_pair, expected_code) in [
        (
            RuntimeWrapperRegisterPair {
                wrapper_name: "  ".into(),
                resource_name: "other".into(),
            },
            "invalid_wrapper_name",
        ),
        (
            RuntimeWrapperRegisterPair {
                wrapper_name: "other".into(),
                resource_name: "  ".into(),
            },
            "invalid_resource_name",
        ),
    ] {
        let req = RuntimeWrapperRegisterRequest {
            wrappers: vec![
                RuntimeWrapperRegisterPair {
                    wrapper_name: "wrapper".into(),
                    resource_name: "resource".into(),
                },
                RuntimeWrapperRegisterPair {
                    wrapper_name: "wrapper".into(),
                    resource_name: "resource".into(),
                },
                invalid_pair,
            ],
        };

        let body = response_body_json(validate_register_request(&req).unwrap_err());
        assert_eq!(body["__status"], 400);
        assert_eq!(body["code"], expected_code);
    }
}

#[test]
fn validate_register_request_accepts_exact_string_near_misses() {
    let req = RuntimeWrapperRegisterRequest {
        wrappers: vec![
            RuntimeWrapperRegisterPair {
                wrapper_name: "shared-wrapper".into(),
                resource_name: "resource-a".into(),
            },
            RuntimeWrapperRegisterPair {
                wrapper_name: "shared-wrapper".into(),
                resource_name: "resource-b".into(),
            },
            RuntimeWrapperRegisterPair {
                wrapper_name: "other-wrapper".into(),
                resource_name: "resource-a".into(),
            },
            RuntimeWrapperRegisterPair {
                wrapper_name: "shared-wrapper".into(),
                resource_name: "resource-a ".into(),
            },
            RuntimeWrapperRegisterPair {
                wrapper_name: "Shared-Wrapper".into(),
                resource_name: "resource-a".into(),
            },
        ],
    };

    validate_register_request(&req).unwrap();
}

#[test]
fn compute_register_diff_resolves_known_and_flags_missing() {
    let creator = EntityCreator::ClientInstance { id: "inst".into() };
    let mut resources = ResourcesDocument::default();
    resources
        .insert(ResourceRecord {
            id: "res_alpha".into(),
            name: "alpha".into(),
            created_by: creator.clone(),
            created_at: 0,
        })
        .unwrap();
    let mut pairs = PairsDocument::default();
    pairs
        .insert(WrapperResourcePairRecord {
            wrapper: "wA".into(),
            resource_id: "res_alpha".into(),
            created_by: creator,
            created_at: 0,
            last_used_at: None,
        })
        .unwrap();

    let req = RuntimeWrapperRegisterRequest {
        wrappers: vec![
            // Already paired — no diff entry.
            RuntimeWrapperRegisterPair {
                wrapper_name: "wA".into(),
                resource_name: "alpha".into(),
            },
            // Known resource, new wrapper — pair missing only.
            RuntimeWrapperRegisterPair {
                wrapper_name: "wB".into(),
                resource_name: "alpha".into(),
            },
            // Unknown resource — both missing.
            RuntimeWrapperRegisterPair {
                wrapper_name: "wC".into(),
                resource_name: "beta".into(),
            },
        ],
    };
    let diff = compute_register_diff(&req, &resources, &pairs);

    assert_eq!(
        diff.resolved_resource_ids.get("alpha").map(String::as_str),
        Some("res_alpha"),
    );
    assert!(!diff.resolved_resource_ids.contains_key("beta"));
    assert_eq!(
        diff.missing_resources.iter().cloned().collect::<Vec<_>>(),
        vec!["beta".to_string()],
    );
    assert_eq!(diff.missing_pairs.len(), 2);
    assert!(
        diff.missing_pairs
            .iter()
            .any(|p| p.wrapper_name == "wB" && p.resource_name == "alpha"),
    );
    assert!(
        diff.missing_pairs
            .iter()
            .any(|p| p.wrapper_name == "wC" && p.resource_name == "beta"),
    );
}

#[test]
fn compute_register_diff_empty_request_is_empty() {
    let resources = ResourcesDocument::default();
    let pairs = PairsDocument::default();
    let req = RuntimeWrapperRegisterRequest { wrappers: vec![] };
    let diff = compute_register_diff(&req, &resources, &pairs);
    assert!(diff.is_empty());
}

#[test]
fn apply_auto_create_inserts_resources_and_pairs() {
    let mut resources = ResourcesDocument::default();
    let mut pairs = PairsDocument::default();
    let mut diff = RegisterDiff::default();
    diff.missing_resources.insert("beta".into());
    diff.missing_pairs.push(RuntimeWrapperRegisterPair {
        wrapper_name: "wC".into(),
        resource_name: "beta".into(),
    });
    let creator = EntityCreator::ClientInstance { id: "inst".into() };

    let (created_resources, created_pairs) =
        apply_auto_create(&mut resources, &mut pairs, &mut diff, &creator, 42).unwrap();

    assert_eq!(created_resources, vec!["beta".to_string()]);
    assert_eq!(created_pairs.len(), 1);
    assert_eq!(created_pairs[0].wrapper_name, "wC");
    let resolved_id = diff.resolved_resource_ids.get("beta").cloned().unwrap();
    assert!(resolved_id.starts_with("res_"));
    assert!(resources.find_by_name("beta").is_some());
    assert!(pairs.find("wC", &resolved_id).is_some());
}

#[test]
fn apply_auto_create_returns_error_for_inconsistent_missing_pair_diff() {
    let creator = EntityCreator::ClientInstance { id: "inst".into() };
    let mut resources = ResourcesDocument::default();
    resources
        .insert(ResourceRecord {
            id: "res_beta".into(),
            name: "beta".into(),
            created_by: creator.clone(),
            created_at: 0,
        })
        .unwrap();
    let mut pairs = PairsDocument::default();
    pairs
        .insert(WrapperResourcePairRecord {
            wrapper: "wrapper".into(),
            resource_id: "res_beta".into(),
            created_by: creator.clone(),
            created_at: 0,
            last_used_at: None,
        })
        .unwrap();
    let mut diff = RegisterDiff::default();
    diff.resolved_resource_ids
        .insert("beta".into(), "res_beta".into());
    diff.missing_pairs.push(RuntimeWrapperRegisterPair {
        wrapper_name: "wrapper".into(),
        resource_name: "beta".into(),
    });

    let error = apply_auto_create(&mut resources, &mut pairs, &mut diff, &creator, 42)
        .expect_err("duplicate missing-pair diff must be an internal error");
    let body = response_body_json(error);
    assert_eq!(body["__status"], 500);
    assert_eq!(body["code"], "internal");
}

#[test]
fn seed_locked_permissions_appends_only_missing_rows() {
    let mut policy = PolicyDocument::default();
    let req = RuntimeWrapperRegisterRequest {
        wrappers: vec![
            RuntimeWrapperRegisterPair {
                wrapper_name: "w1".into(),
                resource_name: "alpha".into(),
            },
            RuntimeWrapperRegisterPair {
                wrapper_name: "w2".into(),
                resource_name: "alpha".into(),
            },
        ],
    };
    let mut resolved = HashMap::new();
    resolved.insert("alpha".to_string(), "res_alpha".to_string());

    let dirty = seed_locked_permissions(&mut policy, &req, &resolved, "inst");
    assert!(dirty);
    assert_eq!(policy.permissions.len(), 2);

    // Re-running with the same input is a no-op.
    let dirty = seed_locked_permissions(&mut policy, &req, &resolved, "inst");
    assert!(!dirty);
    assert_eq!(policy.permissions.len(), 2);
}

#[test]
fn seed_locked_permissions_skips_unresolved_resources() {
    let mut policy = PolicyDocument::default();
    let req = RuntimeWrapperRegisterRequest {
        wrappers: vec![RuntimeWrapperRegisterPair {
            wrapper_name: "w".into(),
            resource_name: "beta".into(),
        }],
    };
    let resolved = HashMap::new();
    let dirty = seed_locked_permissions(&mut policy, &req, &resolved, "inst");
    assert!(!dirty);
    assert!(policy.permissions.is_empty());
}

// ===== End-to-end driver tests =====

#[test]
fn runtime_wrapper_register_rejects_duplicates_before_storage_operations() {
    let fx = make_runtime_api_fixture();
    let store = TestStore::new();
    let request = RuntimeWrapperRegisterRequest {
        wrappers: vec![
            RuntimeWrapperRegisterPair {
                wrapper_name: "valid-wrapper".into(),
                resource_name: "valid-resource".into(),
            },
            RuntimeWrapperRegisterPair {
                wrapper_name: "duplicate-wrapper".into(),
                resource_name: "duplicate-resource".into(),
            },
            RuntimeWrapperRegisterPair {
                wrapper_name: "duplicate-wrapper".into(),
                resource_name: "duplicate-resource".into(),
            },
        ],
    };

    let error = handle_runtime_wrapper_register(&fx.state, &store, &fx.instance_id, &request)
        .expect_err("duplicate request must fail");
    assert_eq!(
        response_body_json(error)["code"],
        "duplicate_wrapper_resource_pair"
    );
    assert!(store.operation_trace().is_empty());
}

#[test]
fn runtime_wrapper_register_seeds_pair_when_auto_create_enabled() {
    use underwrap_protocol::service::{
        RuntimeWrapperRegisterPair, RuntimeWrapperRegisterRequest, RuntimeWrapperRegisterResponse,
    };

    let fx = make_runtime_api_fixture();
    // Flip auto-create on for this instance.
    {
        let mut instances = fx.state.shared.client_instances.write().unwrap();
        instances
            .get_mut(&fx.instance_id)
            .unwrap()
            .auto_create_wrapper_assets = true;
    }
    let store = fx.store();
    let resp = handle_runtime_wrapper_register(
        &fx.state,
        store.as_ref(),
        &fx.instance_id,
        &RuntimeWrapperRegisterRequest {
            wrappers: vec![RuntimeWrapperRegisterPair {
                wrapper_name: "newcomer".into(),
                resource_name: "beta".into(),
            }],
        },
    );
    let parsed: RuntimeWrapperRegisterResponse = typed_response_body(resp);
    match parsed {
        RuntimeWrapperRegisterResponse::Ok {
            auto_created_resources,
            auto_created_pairs,
        } => {
            assert_eq!(auto_created_resources, vec!["beta".to_string()]);
            assert_eq!(auto_created_pairs.len(), 1);
            assert_eq!(auto_created_pairs[0].wrapper_name, "newcomer");
            assert_eq!(auto_created_pairs[0].resource_name, "beta");
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn runtime_wrapper_register_refuses_when_auto_create_disabled() {
    use underwrap_protocol::service::{
        RuntimeWrapperRegisterPair, RuntimeWrapperRegisterRequest, RuntimeWrapperRegisterResponse,
    };

    let fx = make_runtime_api_fixture();
    let store = fx.store();
    let resp = handle_runtime_wrapper_register(
        &fx.state,
        store.as_ref(),
        &fx.instance_id,
        &RuntimeWrapperRegisterRequest {
            wrappers: vec![RuntimeWrapperRegisterPair {
                wrapper_name: "newcomer".into(),
                resource_name: "gamma".into(),
            }],
        },
    );
    let parsed: RuntimeWrapperRegisterResponse = typed_response_body(resp);
    match parsed {
        RuntimeWrapperRegisterResponse::AutoCreateDisabled {
            missing_resources,
            missing_pairs,
        } => {
            assert_eq!(missing_resources, vec!["gamma".to_string()]);
            assert_eq!(missing_pairs.len(), 1);
            assert_eq!(missing_pairs[0].wrapper_name, "newcomer");
            assert_eq!(missing_pairs[0].resource_name, "gamma");
        }
        other => panic!("expected AutoCreateDisabled, got {other:?}"),
    }
}
