use super::*;
use std::io::Cursor;

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
