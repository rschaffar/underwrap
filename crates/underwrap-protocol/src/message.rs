//! IPC message types exchanged over Unix domain sockets.
//!
//! All messages use the binary framing/codec defined in [`crate::ipc`].

#[path = "message/backend.rs"]
mod backend;
#[path = "message/backend_mgmt.rs"]
mod backend_mgmt;
#[path = "message/common.rs"]
mod common;
#[path = "message/control.rs"]
mod control;
#[path = "message/directory.rs"]
mod directory;
#[path = "message/errors.rs"]
mod errors;
#[path = "message/secretsd.rs"]
mod secretsd;
#[path = "message/status.rs"]
mod status;
#[path = "message/wrapper.rs"]
mod wrapper;

pub use backend::{BackendRequest, BackendResponse};
pub use backend_mgmt::{BackendExportStatus, BackendMgmtRequest, BackendMgmtResponse};
pub use common::SecretBytes;
pub use control::{ControlRequest, ControlResponse};
pub use directory::{DirectoryRequest, DirectoryResponse};
pub use errors::{BackendErrorCode, DirectoryErrorCode, ErrorCode, SecretsdErrorCode};
pub use secretsd::{SecretsdRequest, SecretsdResponse};
pub use status::{
    CredentialStatus, CredentialUsageStatus, UnlockGrantStatus, UnlockStatus, WrapperStatus,
    WrapperUnlockStatus,
};
pub use wrapper::{ReadDirectoryBinding, WrapperRequest, WrapperResponse};

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
    fn control_status_roundtrip() {
        let resp = ControlResponse::StatusAllOk {
            wrappers: vec!["gh-authorized".to_string()],
            credentials: vec!["github/token".to_string()],
        };
        let encoded = encode_to_vec(&resp).unwrap();
        let parsed: ControlResponse = decode_from_slice(&encoded).unwrap();
        assert_eq!(parsed, resp);
    }

    #[test]
    fn backend_write_roundtrip() {
        let req = BackendRequest::Write {
            secret_id: "github/token".to_string(),
            material: "ghp_test".into(),
        };
        let encoded = encode_to_vec(&req).unwrap();
        let parsed: BackendRequest = decode_from_slice(&encoded).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn backend_management_export_roundtrip() {
        let req = BackendMgmtRequest::ExportSocket {
            store: "work".to_string(),
            socket: "/run/underwrap-work/backend/backend.sock".to_string(),
            mode: 0o660,
        };
        let encoded = encode_to_vec(&req).unwrap();
        let parsed: BackendMgmtRequest = decode_from_slice(&encoded).unwrap();
        assert_eq!(parsed, req);
    }

    #[test]
    fn backend_management_policy_roundtrip() {
        let req = BackendMgmtRequest::PutPolicy {
            store: "work".to_string(),
            document_json: "{\"wrappers\":[],\"rules\":[]}".to_string(),
        };
        let encoded = encode_to_vec(&req).unwrap();
        let parsed: BackendMgmtRequest = decode_from_slice(&encoded).unwrap();
        assert_eq!(parsed, req);
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
