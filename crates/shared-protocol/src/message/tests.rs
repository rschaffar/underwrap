use super::*;
use crate::ipc::{decode_from_slice, encode_to_vec};

#[test]
fn wrapper_request_roundtrip() {
    let req = WrapperRequest::Read {
        scope: Some("session-42".to_string()),
    };
    let encoded = encode_to_vec(&req).unwrap();
    let parsed: WrapperRequest = decode_from_slice(&encoded).unwrap();
    assert_eq!(parsed, req);
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
