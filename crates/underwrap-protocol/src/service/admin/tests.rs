use super::AdminClientInstanceUpdateRequest;
use crate::policy::ClientAccessOverride;
use serde_json::json;

#[test]
fn client_override_update_omission_and_null_leave_the_field_unchanged() {
    for value in [json!({}), json!({"access_override": null})] {
        let request: AdminClientInstanceUpdateRequest = serde_json::from_value(value).unwrap();
        assert_eq!(request.access_override, None);
        assert_eq!(serde_json::to_value(request).unwrap(), json!({}));
    }
}

#[test]
fn client_override_update_preserves_explicit_modes_and_rejects_invalid_objects() {
    for mode in [
        ClientAccessOverride::Individual {},
        ClientAccessOverride::Locked {},
        ClientAccessOverride::Unlocked { writable: false },
        ClientAccessOverride::Unlocked { writable: true },
    ] {
        let value = json!({"access_override": mode});
        let request: AdminClientInstanceUpdateRequest =
            serde_json::from_value(value.clone()).unwrap();
        assert_eq!(request.access_override, Some(mode));
        assert_eq!(serde_json::to_value(request).unwrap(), value);
    }
    for invalid in [
        json!({"kind": "unknown"}),
        json!({"kind": "unlocked"}),
        json!({"kind": "unlocked", "writable": null}),
        json!({"kind": "unlocked", "writable": "true"}),
        json!({"kind": "individual", "writable": false}),
        json!({"kind": "locked", "writable": true}),
        json!(false),
    ] {
        assert!(
            serde_json::from_value::<AdminClientInstanceUpdateRequest>(
                json!({"access_override": invalid})
            )
            .is_err()
        );
    }
}
