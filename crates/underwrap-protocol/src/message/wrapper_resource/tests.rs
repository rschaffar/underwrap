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
fn exact_resource_message_envelopes_are_bounded() {
    let blob = vec![0; crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES];
    assert_eq!(
        encode_to_vec(&WrapperResourceRequest::Checkin { blob: blob.clone() })
            .unwrap()
            .len(),
        crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES + 5
    );
    assert_eq!(
        encode_to_vec(&WrapperResourceResponse::CheckoutOk {
            blob,
            writable: true
        })
        .unwrap()
        .len(),
        crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES + 6
    );
    let oversized = vec![0; crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES + 1];
    assert!(
        encode_to_vec(&WrapperResourceRequest::Checkin {
            blob: oversized.clone()
        })
        .is_err()
    );
    assert!(
        encode_to_vec(&WrapperResourceResponse::CheckoutOk {
            blob: oversized,
            writable: true
        })
        .is_err()
    );
}

#[test]
fn inner_blob_lengths_and_trailing_bytes_are_rejected() {
    let max = crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES as u32;

    let mut short_request = vec![1];
    short_request.extend_from_slice(&2u32.to_be_bytes());
    short_request.push(b'{');
    assert!(decode_from_slice::<WrapperResourceRequest>(&short_request).is_err());

    let mut long_request = vec![1];
    long_request.extend_from_slice(&1u32.to_be_bytes());
    long_request.extend_from_slice(b"{}");
    let error = decode_from_slice::<WrapperResourceRequest>(&long_request).unwrap_err();
    assert!(error.to_string().contains("trailing bytes"));

    let mut oversized_request = vec![1];
    oversized_request.extend_from_slice(&(max + 1).to_be_bytes());
    let error = decode_from_slice::<WrapperResourceRequest>(&oversized_request).unwrap_err();
    assert!(error.to_string().contains("resource blob exceeds"));
    assert!(!error.to_string().contains("truncated payload"));

    let mut oversized_response = vec![0];
    oversized_response.extend_from_slice(&(max + 1).to_be_bytes());
    let error = decode_from_slice::<WrapperResourceResponse>(&oversized_response).unwrap_err();
    assert!(error.to_string().contains("resource blob exceeds"));
    assert!(!error.to_string().contains("truncated payload"));

    let mut short_response = vec![0];
    short_response.extend_from_slice(&2u32.to_be_bytes());
    short_response.push(b'{');
    assert!(decode_from_slice::<WrapperResourceResponse>(&short_response).is_err());

    let mut long_response = vec![0];
    long_response.extend_from_slice(&1u32.to_be_bytes());
    long_response.extend_from_slice(b"{}");
    long_response.push(1);
    let error = decode_from_slice::<WrapperResourceResponse>(&long_response).unwrap_err();
    assert!(error.to_string().contains("invalid bool") || error.to_string().contains("trailing"));
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
