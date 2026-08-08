use super::*;

#[test]
fn exact_frames_round_trip_at_boundaries() {
    for len in [MAX_BODY_BYTES - 1, MAX_BODY_BYTES] {
        let encoded = encode_request_header(7, len).unwrap();
        assert_eq!(
            decode_request_header(&encoded).unwrap(),
            RequestHeader {
                generation: 7,
                body_len: len
            }
        );
    }
    assert!(encode_request_header(0, MAX_BODY_BYTES + 1).is_err());
    for kind in [ResponseKind::Ack, ResponseKind::Reject] {
        assert_eq!(
            decode_response(&encode_response(kind, 9)).unwrap(),
            (kind, 9)
        );
    }
}

#[test]
fn malformed_headers_and_responses_are_rejected() {
    let mut request = encode_request_header(0, 0).unwrap();
    for index in [0, 4, 5] {
        let old = request[index];
        request[index] ^= 0xff;
        assert!(decode_request_header(&request).is_err());
        request[index] = old;
    }
    request[14..18].copy_from_slice(&((MAX_BODY_BYTES + 1) as u32).to_be_bytes());
    assert!(decode_request_header(&request).is_err());

    let mut response = encode_response(ResponseKind::Ack, 0);
    for index in [0, 4, 5] {
        let old = response[index];
        response[index] ^= 0xff;
        assert!(decode_response(&response).is_err());
        response[index] = old;
    }
}
