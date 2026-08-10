use super::*;
use std::io::{Cursor, Error, Write};

struct ExactPayload(usize);

impl WireEncode for ExactPayload {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        out.resize(self.0, 0x5a);
        Ok(())
    }
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Err(Error::other("injected write failure"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TestPayload {
    flag: bool,
    name: String,
    bytes: Vec<u8>,
    count: u64,
    maybe: Option<String>,
}

impl WireEncode for TestPayload {
    fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        self.flag.encode(out)?;
        self.name.encode(out)?;
        self.bytes.encode(out)?;
        self.count.encode(out)?;
        self.maybe.encode(out)
    }
}

impl WireDecode for TestPayload {
    fn decode(reader: &mut WireReader<'_>) -> Result<Self> {
        Ok(Self {
            flag: bool::decode(reader)?,
            name: String::decode(reader)?,
            bytes: Vec::<u8>::decode(reader)?,
            count: u64::decode(reader)?,
            maybe: Option::<String>::decode(reader)?,
        })
    }
}

#[test]
fn roundtrip_sync() {
    let msg = TestPayload {
        flag: true,
        name: "world".to_string(),
        bytes: vec![1, 2, 3, 4],
        count: 42,
        maybe: Some("hello".to_string()),
    };
    let mut buf = Vec::new();
    write_frame_sync(&mut buf, &msg).unwrap();

    let mut cursor = Cursor::new(&buf);
    let decoded: TestPayload = read_frame_sync(&mut cursor).unwrap();
    assert_eq!(msg, decoded);
}

#[tokio::test]
async fn roundtrip_async() {
    let msg = TestPayload {
        flag: false,
        name: "async".to_string(),
        bytes: vec![9, 8, 7],
        count: 99,
        maybe: None,
    };
    let mut buf = Vec::new();
    write_frame_async(&mut buf, &msg).await.unwrap();

    let mut cursor = tokio::io::BufReader::new(&buf[..]);
    let decoded: TestPayload = read_frame_async(&mut cursor).await.unwrap();
    assert_eq!(msg, decoded);
}

#[test]
fn generic_sync_bound_is_inclusive_and_rejects_oversize_before_write() {
    let mut wire = Vec::new();
    write_frame_sync(&mut wire, &ExactPayload(MAX_FRAME_SIZE)).unwrap();
    assert_eq!(wire.len(), MAX_FRAME_SIZE + 4);
    assert!(write_frame_sync(&mut Vec::new(), &ExactPayload(MAX_FRAME_SIZE + 1)).is_err());

    let prefix_only = ((MAX_FRAME_SIZE + 1) as u32).to_be_bytes();
    let error = read_frame_sync::<u8>(&mut Cursor::new(prefix_only)).unwrap_err();
    assert!(error.to_string().contains("frame too large"));
}

#[tokio::test]
async fn generic_async_bound_is_inclusive_and_rejects_declared_oversize() {
    let mut wire = Vec::new();
    write_frame_async(&mut wire, &ExactPayload(MAX_FRAME_SIZE))
        .await
        .unwrap();
    assert_eq!(wire.len(), MAX_FRAME_SIZE + 4);
    assert!(
        write_frame_async(&mut Vec::new(), &ExactPayload(MAX_FRAME_SIZE + 1))
            .await
            .is_err()
    );

    let prefix_only = ((MAX_FRAME_SIZE + 1) as u32).to_be_bytes();
    let mut input = tokio::io::BufReader::new(prefix_only.as_slice());
    let error = read_frame_async::<u8>(&mut input).await.unwrap_err();
    assert!(error.to_string().contains("frame too large"));
}

#[test]
fn wrapper_resource_sync_round_trips_exact_max_request_and_response() {
    use crate::message::{WrapperResourceRequest, WrapperResourceResponse};
    let blob = vec![0; crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES];
    let messages = [
        encode_to_vec(&WrapperResourceRequest::Checkin { blob: blob.clone() })
            .unwrap()
            .len(),
        encode_to_vec(&WrapperResourceResponse::CheckoutOk {
            blob: blob.clone(),
            writable: true,
        })
        .unwrap()
        .len(),
    ];
    assert_eq!(messages, [blob.len() + 5, blob.len() + 6]);

    let mut request_wire = Vec::new();
    write_wrapper_resource_frame_sync(
        &mut request_wire,
        &WrapperResourceRequest::Checkin { blob: blob.clone() },
    )
    .unwrap();
    assert_eq!(request_wire.len(), blob.len() + 5 + 4);
    assert_eq!(
        read_wrapper_resource_frame_sync::<WrapperResourceRequest>(&mut Cursor::new(request_wire))
            .unwrap(),
        WrapperResourceRequest::Checkin { blob: blob.clone() }
    );

    let mut response_wire = Vec::new();
    write_wrapper_resource_frame_sync(
        &mut response_wire,
        &WrapperResourceResponse::CheckoutOk {
            blob: blob.clone(),
            writable: true,
        },
    )
    .unwrap();
    assert_eq!(response_wire.len(), blob.len() + 6 + 4);
    assert_eq!(
        read_wrapper_resource_frame_sync::<WrapperResourceResponse>(&mut Cursor::new(
            response_wire
        ))
        .unwrap(),
        WrapperResourceResponse::CheckoutOk {
            blob: blob.clone(),
            writable: true
        }
    );
    assert!(
        write_frame_sync(
            &mut Vec::new(),
            &WrapperResourceResponse::CheckoutOk {
                blob,
                writable: true
            },
        )
        .is_err()
    );
}

#[tokio::test]
async fn wrapper_resource_async_round_trips_exact_max_request_and_response() {
    use crate::message::{WrapperResourceRequest, WrapperResourceResponse};
    let blob = vec![0; crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES];
    let request = WrapperResourceRequest::Checkin { blob: blob.clone() };
    let mut wire = Vec::new();
    write_wrapper_resource_frame_async(&mut wire, &request)
        .await
        .unwrap();
    let mut input = tokio::io::BufReader::new(wire.as_slice());
    assert_eq!(
        read_wrapper_resource_frame_async::<WrapperResourceRequest>(&mut input)
            .await
            .unwrap(),
        request
    );

    let response = WrapperResourceResponse::CheckoutOk {
        blob,
        writable: false,
    };
    let mut wire = Vec::new();
    write_wrapper_resource_frame_async(&mut wire, &response)
        .await
        .unwrap();
    let mut input = tokio::io::BufReader::new(wire.as_slice());
    assert_eq!(
        read_wrapper_resource_frame_async::<WrapperResourceResponse>(&mut input)
            .await
            .unwrap(),
        response
    );
}

#[test]
fn wrapper_resource_sync_rejects_oversized_inner_blobs_before_body_read() {
    use crate::message::{WrapperResourceRequest, WrapperResourceResponse};

    for (payload, request) in [
        (
            [1u8]
                .into_iter()
                .chain(
                    ((crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES + 1) as u32).to_be_bytes(),
                )
                .collect::<Vec<_>>(),
            true,
        ),
        (
            [0u8]
                .into_iter()
                .chain(
                    ((crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES + 1) as u32).to_be_bytes(),
                )
                .collect::<Vec<_>>(),
            false,
        ),
    ] {
        let mut frame = Vec::from((payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&payload);
        let error = if request {
            read_wrapper_resource_frame_sync::<WrapperResourceRequest>(&mut Cursor::new(frame))
                .unwrap_err()
        } else {
            read_wrapper_resource_frame_sync::<WrapperResourceResponse>(&mut Cursor::new(frame))
                .unwrap_err()
        };
        assert!(error.to_string().contains("resource blob exceeds"));
        assert!(!error.to_string().contains("truncated payload"));
    }
}

#[tokio::test]
async fn wrapper_resource_async_rejects_oversized_inner_blobs_before_body_read() {
    use crate::message::{WrapperResourceRequest, WrapperResourceResponse};

    let request_payload = [1u8]
        .into_iter()
        .chain(((crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES + 1) as u32).to_be_bytes())
        .collect::<Vec<_>>();
    let mut request_frame = Vec::from((request_payload.len() as u32).to_be_bytes());
    request_frame.extend_from_slice(&request_payload);
    let mut request_input = tokio::io::BufReader::new(request_frame.as_slice());
    let error = read_wrapper_resource_frame_async::<WrapperResourceRequest>(&mut request_input)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("resource blob exceeds"));
    assert!(!error.to_string().contains("truncated payload"));

    let response_payload = [0u8]
        .into_iter()
        .chain(((crate::resource_blob::MAX_CANONICAL_RESOURCE_BYTES + 1) as u32).to_be_bytes())
        .collect::<Vec<_>>();
    let mut response_frame = Vec::from((response_payload.len() as u32).to_be_bytes());
    response_frame.extend_from_slice(&response_payload);
    let mut response_input = tokio::io::BufReader::new(response_frame.as_slice());
    let error = read_wrapper_resource_frame_async::<WrapperResourceResponse>(&mut response_input)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("resource blob exceeds"));
    assert!(!error.to_string().contains("truncated payload"));
}

#[tokio::test]
async fn wrapper_resource_async_frame_rejects_declared_over_limit_before_body_read() {
    let declared = (MAX_WRAPPER_RESOURCE_FRAME_SIZE + 1) as u32;
    let prefix = declared.to_be_bytes();
    let mut input = tokio::io::BufReader::new(prefix.as_slice());
    let error = read_wrapper_resource_frame_async::<u8>(&mut input)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("frame too large"));
}

#[test]
fn wrapper_resource_sync_rejects_declared_over_limit_and_truncated_frames() {
    let prefix = ((MAX_WRAPPER_RESOURCE_FRAME_SIZE + 1) as u32).to_be_bytes();
    let error = read_wrapper_resource_frame_sync::<u8>(&mut Cursor::new(prefix)).unwrap_err();
    assert!(error.to_string().contains("frame too large"));

    assert!(read_wrapper_resource_frame_sync::<u8>(&mut Cursor::new([0u8; 3])).is_err());
    let mut truncated_body = Vec::from(2u32.to_be_bytes());
    truncated_body.push(1);
    assert!(read_wrapper_resource_frame_sync::<u8>(&mut Cursor::new(truncated_body)).is_err());
}

#[tokio::test]
async fn wrapper_resource_async_rejects_truncated_prefix_and_body() {
    let mut prefix = tokio::io::BufReader::new(&[0u8; 3][..]);
    assert!(
        read_wrapper_resource_frame_async::<u8>(&mut prefix)
            .await
            .is_err()
    );
    let mut bytes = Vec::from(2u32.to_be_bytes());
    bytes.push(1);
    let mut body = tokio::io::BufReader::new(bytes.as_slice());
    assert!(
        read_wrapper_resource_frame_async::<u8>(&mut body)
            .await
            .is_err()
    );
}

#[test]
fn frame_write_failures_and_oversize_fail_deterministically() {
    assert!(write_frame_sync(&mut FailingWriter, &true).is_err());
    assert!(
        write_wrapper_resource_frame_sync(
            &mut Vec::new(),
            &ExactPayload(MAX_WRAPPER_RESOURCE_FRAME_SIZE + 1),
        )
        .is_err()
    );
}

#[test]
fn decode_rejects_trailing_bytes() {
    let mut payload = encode_to_vec(&true).unwrap();
    payload.push(0xAA);
    let err = decode_from_slice::<bool>(&payload).unwrap_err();
    assert!(err.to_string().contains("trailing bytes"));
}

#[test]
fn decode_rejects_invalid_bool() {
    let err = decode_from_slice::<bool>(&[2]).unwrap_err();
    assert!(err.to_string().contains("invalid bool discriminant"));
}

#[test]
fn inflated_vec_length_does_not_oom() {
    let mut payload = Vec::new();
    0xFFFF_FFFFu32.encode(&mut payload).unwrap();
    let err = decode_from_slice::<Vec<u8>>(&payload).unwrap_err();
    assert!(err.to_string().contains("truncated payload"));
}

#[test]
fn inflated_map_length_does_not_oom() {
    let mut payload = Vec::new();
    0xFFFF_FFFFu32.encode(&mut payload).unwrap();
    let err = decode_from_slice::<HashMap<String, u8>>(&payload).unwrap_err();
    assert!(err.to_string().contains("truncated payload"));
}
