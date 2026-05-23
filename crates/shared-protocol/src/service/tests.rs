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
