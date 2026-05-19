//! IPC message types exchanged over Unix domain sockets.
//!
//! All messages use the binary framing/codec defined in [`crate::ipc`].

#[path = "message/common.rs"]
mod common;
#[path = "message/directory.rs"]
mod directory;
#[path = "message/errors.rs"]
mod errors;
#[path = "message/secretsd.rs"]
mod secretsd;
#[path = "message/wrapper.rs"]
mod wrapper;
#[path = "message/wrapper_resource.rs"]
mod wrapper_resource;

pub use common::SecretBytes;
pub use directory::{DirectoryRequest, DirectoryResponse};
pub use errors::{DirectoryErrorCode, ErrorCode, SecretsdErrorCode};
pub use secretsd::{SecretsdRequest, SecretsdResponse};
pub use wrapper::{ReadDirectoryBinding, ReadResourceBinding, WrapperRequest, WrapperResponse};
pub use wrapper_resource::{ResourceErrorCode, WrapperResourceRequest, WrapperResourceResponse};

#[cfg(test)]
mod tests {
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

    #[test]
    fn secretsd_response_roundtrip() {
        let resp = SecretsdResponse::ReadOk {
            material: Some("token".into()),
        };
        let encoded = encode_to_vec(&resp).unwrap();
        let parsed: SecretsdResponse = decode_from_slice(&encoded).unwrap();
        assert_eq!(parsed, resp);
    }
}
