use super::*;
use crate::ipc::{decode_from_slice, encode_to_vec};

#[test]
fn checkout_roundtrips() {
    let req = WrapperResourceRequest::Checkout;
    let bytes = encode_to_vec(&req).unwrap();
    let back: WrapperResourceRequest = decode_from_slice(&bytes).unwrap();
    assert_eq!(back, req);
}

#[test]
fn checkin_roundtrips() {
    let req = WrapperResourceRequest::Checkin {
        blob: b"\x00\x01canonical-json-bytes".to_vec(),
    };
    let bytes = encode_to_vec(&req).unwrap();
    let back: WrapperResourceRequest = decode_from_slice(&bytes).unwrap();
    assert_eq!(back, req);
}

#[test]
fn usage_signal_roundtrips() {
    let req = WrapperResourceRequest::UsageSignal;
    let bytes = encode_to_vec(&req).unwrap();
    let back: WrapperResourceRequest = decode_from_slice(&bytes).unwrap();
    assert_eq!(back, req);
}

#[test]
fn checkout_ok_response_roundtrips() {
    let resp = WrapperResourceResponse::CheckoutOk {
        blob: b"{}".to_vec(),
        writable: true,
    };
    let bytes = encode_to_vec(&resp).unwrap();
    let back: WrapperResourceResponse = decode_from_slice(&bytes).unwrap();
    assert_eq!(back, resp);
}

#[test]
fn error_response_roundtrips() {
    let resp = WrapperResourceResponse::Error {
        code: ResourceErrorCode::Unauthorized,
        message: "writes not permitted by grant".to_string(),
    };
    let bytes = encode_to_vec(&resp).unwrap();
    let back: WrapperResourceResponse = decode_from_slice(&bytes).unwrap();
    assert_eq!(back, resp);
}

#[test]
fn unknown_request_discriminant_rejected() {
    let bad = vec![9u8];
    let err = decode_from_slice::<WrapperResourceRequest>(&bad).unwrap_err();
    assert!(format!("{err:#}").contains("invalid wrapper-resource request"));
}
