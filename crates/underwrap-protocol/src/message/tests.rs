use super::*;
use crate::ipc::{decode_from_slice, encode_to_vec};
use crate::variant::WrapperVariant;

#[test]
fn wrapper_request_real_wire_bytes_are_exact() {
    let req = WrapperRequest::Read {
        wrapper_name: "gh-authorized".to_string(),
        variant: WrapperVariant::Real,
    };
    let encoded = encode_to_vec(&req).unwrap();
    assert_eq!(
        encoded,
        [
            0x00, 0x00, 0x00, 0x00, 0x0d, b'g', b'h', b'-', b'a', b'u', b't', b'h', b'o', b'r',
            b'i', b'z', b'e', b'd', 0x00,
        ]
    );
    let parsed: WrapperRequest = decode_from_slice(&encoded).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn wrapper_request_rejects_legacy_scope_as_trailing_bytes() {
    let mut encoded = vec![
        0x00, 0x00, 0x00, 0x00, 0x0d, b'g', b'h', b'-', b'a', b'u', b't', b'h', b'o', b'r', b'i',
        b'z', b'e', b'd', 0x00,
    ];
    encoded.extend_from_slice(&[
        0x01, 0x00, 0x00, 0x00, 0x0a, b's', b'e', b's', b's', b'i', b'o', b'n', b'-', b'4', b'2',
    ]);
    let error = decode_from_slice::<WrapperRequest>(&encoded).unwrap_err();
    assert_eq!(error.to_string(), "trailing bytes in payload");
}

#[test]
fn wrapper_request_debug_variant_roundtrip() {
    let req = WrapperRequest::Read {
        wrapper_name: "gh-authorized".to_string(),
        variant: WrapperVariant::Debug,
    };
    let encoded = encode_to_vec(&req).unwrap();
    let parsed: WrapperRequest = decode_from_slice(&encoded).unwrap();
    assert_eq!(parsed, req);
}

#[test]
fn wrapper_response_readok_roundtrip() {
    let resp = WrapperResponse::ReadOk {
        env_projection: true,
        resource_id: "res_abc".to_string(),
        resource_name: "github/token".to_string(),
        access: crate::access::AccessMode::ReadWrite,
        libsecret: false,
        files_target_paths: vec![],
        target: std::path::PathBuf::from("/run/current-system/sw/bin/gh"),
        env_passthrough: vec!["TERM".to_string()],
        env_set: std::collections::HashMap::from([("PATH".to_string(), "/bin".to_string())]),
        target_runtime_dir_base: "/run/user".to_string(),
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
