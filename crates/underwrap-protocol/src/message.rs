//! IPC message types exchanged over Unix domain sockets.
//!
//! All messages are length-prefixed JSON frames:
//!   [4 bytes big-endian length][JSON payload]

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Read-path messages (wrapper -> daemon over read.sock)
// ---------------------------------------------------------------------------

/// A wrapper requesting secret material from the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadRequest {
    /// The wrapper name as declared in configuration (e.g. "gh-authorized").
    pub wrapper: String,
    /// Optional scope identifier (e.g. shell session id).
    pub scope: Option<String>,
}

/// Daemon response to a read request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ReadResponse {
    #[serde(rename = "ok")]
    Ok {
        /// The secret material (plaintext token, derived access token, etc.).
        material: String,
    },
    #[serde(rename = "error")]
    Error { code: ErrorCode, message: String },
}

// ---------------------------------------------------------------------------
// Control-path messages (underwrapctl -> daemon over control.sock)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command")]
pub enum ControlRequest {
    #[serde(rename = "status")]
    Status {
        /// Optional: query status for a specific wrapper or secret.
        target: Option<String>,
    },
    #[serde(rename = "store")]
    Store { secret_id: String, material: String },
    /// Unlock a wrapper for a duration.
    #[serde(rename = "unlock")]
    Unlock {
        /// Wrapper name to unlock.
        target: String,
        /// Optional scope (e.g. shell session id).
        scope: Option<String>,
        /// Seconds until auto-lock. None = indefinite (until explicit lock).
        seconds: Option<u64>,
    },
    /// Explicitly lock a wrapper.
    #[serde(rename = "lock")]
    Lock {
        /// Wrapper name to lock. None = lock everything.
        target: Option<String>,
        /// Optional scope. None = all scopes.
        scope: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ControlResponse {
    #[serde(rename = "ok")]
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        info: Option<serde_json::Value>,
    },
    #[serde(rename = "error")]
    Error { code: ErrorCode, message: String },
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorCode {
    /// Caller is not authorized for this operation.
    Unauthorized,
    /// The requested wrapper or secret does not exist.
    NotFound,
    /// The wrapper/secret is locked and requires unlock first.
    Locked,
    /// The derivation (e.g. OAuth refresh) failed.
    DerivationFailed,
    /// Internal daemon error.
    Internal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_request_roundtrip() {
        let req = ReadRequest {
            wrapper: "gh-authorized".to_string(),
            scope: Some("session-42".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: ReadRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.wrapper, "gh-authorized");
        assert_eq!(parsed.scope.as_deref(), Some("session-42"));
    }

    #[test]
    fn read_response_ok() {
        let resp = ReadResponse::Ok {
            material: "secret123".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"material\":\"secret123\""));
    }

    #[test]
    fn read_response_error() {
        let resp = ReadResponse::Error {
            code: ErrorCode::NotFound,
            message: "nope".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"error\""));
        let parsed: ReadResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ReadResponse::Error { .. }));
    }

    #[test]
    fn control_request_store_roundtrip() {
        let req = ControlRequest::Store {
            secret_id: "a/b".to_string(),
            material: "val".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"command\":\"store\""));
        let parsed: ControlRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            ControlRequest::Store {
                secret_id,
                material,
            } => {
                assert_eq!(secret_id, "a/b");
                assert_eq!(material, "val");
            }
            other => panic!("expected Store, got {other:?}"),
        }
    }

    #[test]
    fn control_response_ok_with_info() {
        let resp = ControlResponse::Ok {
            info: Some(serde_json::json!({"key": "val"})),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"key\":\"val\""));
    }

    #[test]
    fn control_response_ok_without_info() {
        let resp = ControlResponse::Ok { info: None };
        let json = serde_json::to_string(&resp).unwrap();
        // "info" should be absent due to skip_serializing_if.
        assert!(!json.contains("\"info\""));
    }
}
