//! IPC message types exchanged over Unix domain sockets.
//!
//! All messages are length-prefixed JSON frames:
//!   [4 bytes big-endian length][JSON payload]

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Read-path messages (wrapper -> daemon over read.sock)
// ---------------------------------------------------------------------------

/// Request sent by a wrapper over the read socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum WrapperRequest {
    /// Request secret material.
    #[serde(rename = "read")]
    Read {
        wrapper: String,
        scope: Option<String>,
    },
    /// Request unlock — initiates SSH agent challenge-response.
    #[serde(rename = "unlock")]
    Unlock {
        wrapper: String,
        scope: Option<String>,
        seconds: Option<u64>,
    },
    /// Complete unlock by providing the signed challenge.
    #[serde(rename = "unlock_response")]
    UnlockResponse {
        wrapper: String,
        scope: Option<String>,
        seconds: Option<u64>,
        /// The challenge nonce (hex-encoded, echoed back).
        challenge: String,
        /// The SSH signature over the challenge (base64-encoded).
        signature: String,
        /// The SSH public key that signed (OpenSSH format).
        public_key: String,
    },
}

/// Daemon response on the read socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum WrapperResponse {
    #[serde(rename = "ok")]
    Ok {
        /// Secret material (for read requests).
        #[serde(skip_serializing_if = "Option::is_none")]
        material: Option<String>,
        /// Message (for unlock confirmations).
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// SSH challenge: caller must sign this with their SSH agent.
    #[serde(rename = "challenge")]
    Challenge {
        /// Random nonce (hex-encoded) to be signed.
        nonce: String,
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

// ---------------------------------------------------------------------------
// Backend protocol (frontend daemon <-> backend daemon over tunnel socket)
// ---------------------------------------------------------------------------

/// Request from the frontend daemon to the remote backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum BackendRequest {
    /// Read secret material (raw or derived).
    #[serde(rename = "read")]
    Read { secret_id: String },

    /// Read and derive (e.g. OAuth refresh → access token).
    #[serde(rename = "derive")]
    Derive {
        secret_id: String,
        /// Derivation mode name (e.g. "oauth_refresh_to_access_token").
        mode: String,
    },

    /// Write/update secret material.
    #[serde(rename = "write")]
    Write { secret_id: String, material: String },

    /// Check backend health / list available secrets.
    #[serde(rename = "status")]
    Status,
}

/// Response from the remote backend to the frontend daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum BackendResponse {
    #[serde(rename = "ok")]
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        material: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        info: Option<serde_json::Value>,
    },
    #[serde(rename = "error")]
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_read_request_roundtrip() {
        let req = WrapperRequest::Read {
            wrapper: "gh-authorized".to_string(),
            scope: Some("session-42".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: WrapperRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            WrapperRequest::Read { wrapper, scope } => {
                assert_eq!(wrapper, "gh-authorized");
                assert_eq!(scope.as_deref(), Some("session-42"));
            }
            other => panic!("expected Read, got {other:?}"),
        }
    }

    #[test]
    fn wrapper_response_ok() {
        let resp = WrapperResponse::Ok {
            material: Some("secret123".to_string()),
            message: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"material\":\"secret123\""));
    }

    #[test]
    fn wrapper_response_error() {
        let resp = WrapperResponse::Error {
            code: ErrorCode::NotFound,
            message: "nope".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"error\""));
        let parsed: WrapperResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, WrapperResponse::Error { .. }));
    }

    #[test]
    fn wrapper_response_challenge() {
        let resp = WrapperResponse::Challenge {
            nonce: "abc123".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"challenge\""));
        assert!(json.contains("\"nonce\":\"abc123\""));
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
        assert!(!json.contains("\"info\""));
    }

    #[test]
    fn backend_read_roundtrip() {
        let req = BackendRequest::Read {
            secret_id: "github/token".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"action\":\"read\""));
        let parsed: BackendRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            BackendRequest::Read { secret_id } => assert_eq!(secret_id, "github/token"),
            other => panic!("expected Read, got {other:?}"),
        }
    }

    #[test]
    fn backend_derive_roundtrip() {
        let req = BackendRequest::Derive {
            secret_id: "google/creds".to_string(),
            mode: "oauth_refresh_to_access_token".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"action\":\"derive\""));
        let parsed: BackendRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            BackendRequest::Derive { secret_id, mode } => {
                assert_eq!(secret_id, "google/creds");
                assert_eq!(mode, "oauth_refresh_to_access_token");
            }
            other => panic!("expected Derive, got {other:?}"),
        }
    }

    #[test]
    fn backend_write_roundtrip() {
        let req = BackendRequest::Write {
            secret_id: "tok".to_string(),
            material: "secret_value".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"action\":\"write\""));
        // Material should be present in JSON (backend protocol, not user-facing logs).
        assert!(json.contains("\"material\":\"secret_value\""));
    }

    #[test]
    fn backend_response_ok_with_material() {
        let resp = BackendResponse::Ok {
            material: Some("derived_token".to_string()),
            info: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"material\":\"derived_token\""));
        assert!(!json.contains("\"info\""));
    }

    #[test]
    fn backend_response_error() {
        let resp = BackendResponse::Error {
            message: "not found".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"error\""));
    }
}
