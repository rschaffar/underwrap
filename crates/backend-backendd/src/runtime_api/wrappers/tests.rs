use super::*;
use crate::test_support::{make_runtime_api_fixture, response_body_json};

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
    let parsed: RuntimeWrapperRegisterResponse =
        serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
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
    let parsed: RuntimeWrapperRegisterResponse =
        serde_json::from_slice(&response_body_json(resp).to_string().into_bytes()).unwrap();
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
