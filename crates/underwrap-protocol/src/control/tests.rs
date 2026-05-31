use super::*;
use crate::entities::{PAIRS_DOCUMENT_SECRET_ID, RESOURCES_DOCUMENT_SECRET_ID};
use crate::policy::POLICY_DOCUMENT_SECRET_ID;

#[test]
fn control_documents_are_control_keys() {
    assert!(is_control_key(POLICY_DOCUMENT_SECRET_ID));
    assert!(is_control_key(RESOURCES_DOCUMENT_SECRET_ID));
    assert!(is_control_key(PAIRS_DOCUMENT_SECRET_ID));
    assert!(is_control_key("control/canary"));
}

#[test]
fn resource_keys_are_not_control_keys() {
    // Resource blobs and their flat-KV entry ids (server-generated
    // `res_*` ids) never start with the control prefix.
    assert!(!is_control_key("resources/res_abc123"));
    assert!(!is_control_key("res_abc123/env/TOKEN"));
    assert!(!is_control_key("res_abc123/files/.config/gh/hosts.yml"));
}

#[test]
fn all_control_documents_share_the_prefix() {
    for id in [
        POLICY_DOCUMENT_SECRET_ID,
        RESOURCES_DOCUMENT_SECRET_ID,
        PAIRS_DOCUMENT_SECRET_ID,
    ] {
        assert!(
            id.starts_with(CONTROL_PREFIX),
            "{id} must live under {CONTROL_PREFIX}"
        );
    }
}
