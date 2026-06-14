use super::*;
use crate::ipc::{decode_from_slice, encode_to_vec};
use crate::variant::WrapperVariant;

#[test]
fn wrapper_request_roundtrip() {
    let req = WrapperRequest::Read {
        wrapper_name: "gh-authorized".to_string(),
        variant: WrapperVariant::Real,
        scope: Some("session-42".to_string()),
    };
    let encoded = encode_to_vec(&req).unwrap();
    let parsed: WrapperRequest = decode_from_slice(&encoded).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn wrapper_request_debug_variant_roundtrip() {
    let req = WrapperRequest::Read {
        wrapper_name: "gh-authorized".to_string(),
        variant: WrapperVariant::Debug,
        scope: None,
    };
    let encoded = encode_to_vec(&req).unwrap();
    let parsed: WrapperRequest = decode_from_slice(&encoded).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn wrapper_response_readok_roundtrip() {
    let resp = WrapperResponse::ReadOk {
        env: std::collections::HashMap::new(),
        resource_id: "res_abc".to_string(),
        resource_name: "github/token".to_string(),
        access: crate::config::AccessMode::ReadWrite,
        libsecret: false,
        files_target_paths: vec![],
        target: std::path::PathBuf::from("/run/current-system/sw/bin/gh"),
        env_passthrough: vec!["TERM".to_string()],
        env_set: std::collections::HashMap::from([("PATH".to_string(), "/bin".to_string())]),
        xdg_runtime_dir_base: "/run/user".to_string(),
        access_paths: vec!["~/.config/gh".to_string()],
    };
    let encoded = encode_to_vec(&resp).unwrap();
    let parsed: WrapperResponse = decode_from_slice(&encoded).unwrap();
    assert_eq!(parsed, resp);
}

#[test]
fn wrapper_response_roundtrip() {
    let resp = WrapperResponse::Error {
        code: ErrorCode::NotFound,
        message: "missing secret".to_string(),
    };
    let encoded = encode_to_vec(&resp).unwrap();
    let parsed: WrapperResponse = decode_from_slice(&encoded).unwrap();
    assert_eq!(parsed, resp);
}
