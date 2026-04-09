//! IPC message types exchanged over Unix domain sockets.
//!
//! All messages are length-prefixed JSON frames:
//!   [4 bytes big-endian length][JSON payload]

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop, PartialEq, Eq)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn as_utf8(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }

    pub fn into_utf8_string(mut self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(std::mem::take(&mut self.0))
    }
}

impl std::ops::Deref for SecretBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<[u8]> for SecretBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for SecretBytes {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<String> for SecretBytes {
    fn from(value: String) -> Self {
        Self(value.into_bytes())
    }
}

impl From<&str> for SecretBytes {
    fn from(value: &str) -> Self {
        Self(value.as_bytes().to_vec())
    }
}

impl From<SecretBytes> for Vec<u8> {
    fn from(mut value: SecretBytes) -> Self {
        std::mem::take(&mut value.0)
    }
}

impl PartialEq<&str> for SecretBytes {
    fn eq(&self, other: &&str) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<SecretBytes> for &str {
    fn eq(&self, other: &SecretBytes) -> bool {
        self.as_bytes() == other.0.as_slice()
    }
}

impl PartialEq<String> for SecretBytes {
    fn eq(&self, other: &String) -> bool {
        self.0 == other.as_bytes()
    }
}

impl PartialEq<SecretBytes> for String {
    fn eq(&self, other: &SecretBytes) -> bool {
        self.as_bytes() == other.0.as_slice()
    }
}

impl<const N: usize> PartialEq<&[u8; N]> for SecretBytes {
    fn eq(&self, other: &&[u8; N]) -> bool {
        self.0 == other.as_slice()
    }
}

impl<const N: usize> PartialEq<SecretBytes> for &[u8; N] {
    fn eq(&self, other: &SecretBytes) -> bool {
        self.as_slice() == other.0.as_slice()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop, PartialEq, Eq)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for SecretString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for SecretString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<SecretString> for String {
    fn from(mut value: SecretString) -> Self {
        std::mem::take(&mut value.0)
    }
}

impl PartialEq<&str> for SecretString {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<SecretString> for &str {
    fn eq(&self, other: &SecretString) -> bool {
        *self == other.0
    }
}

impl PartialEq<String> for SecretString {
    fn eq(&self, other: &String) -> bool {
        self.0 == *other
    }
}

impl PartialEq<SecretString> for String {
    fn eq(&self, other: &SecretString) -> bool {
        *self == other.0
    }
}

// ---------------------------------------------------------------------------
// Wrapper-socket messages (wrapper -> daemon over wrapper socket)
// ---------------------------------------------------------------------------

/// Request sent by a wrapper over its wrapper socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum WrapperRequest {
    /// Request secret material.
    #[serde(rename = "read")]
    Read { scope: Option<String> },
    /// Request unlock — initiates SSH agent challenge-response.
    #[serde(rename = "unlock")]
    Unlock {
        scope: Option<String>,
        seconds: Option<u64>,
    },
    /// Complete unlock by providing the signed challenge.
    #[serde(rename = "unlock_response")]
    UnlockResponse {
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

/// Daemon response on the wrapper socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum WrapperResponse {
    #[serde(rename = "ok")]
    Ok {
        /// Environment variable bindings for read requests.
        #[serde(skip_serializing_if = "Option::is_none")]
        env: Option<std::collections::HashMap<String, SecretString>>,
        /// Backward-compat/testing convenience for single env binding cases.
        #[serde(skip_serializing_if = "Option::is_none")]
        material: Option<SecretString>,
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
    Store {
        secret_id: String,
        material: SecretString,
    },
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
// Secretsd channel messages (daemon <-> secretsd over authenticated socketpair)
// ---------------------------------------------------------------------------

/// Request from secretsd to daemon over the authenticated socketpair channel.
/// All secret_id values are relative — the daemon handler prepends the
/// wrapper's prefix (e.g., `<wrapper>/libsecret/`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum SecretsdRequest {
    /// Read a secret by relative ID.
    #[serde(rename = "read")]
    Read { secret_id: String },
    /// Write a secret by relative ID.
    #[serde(rename = "write")]
    Write {
        secret_id: String,
        material: SecretString,
    },
    /// List secrets under a relative prefix.
    #[serde(rename = "list")]
    List { prefix: String },
    /// Delete a secret by relative ID.
    #[serde(rename = "delete")]
    Delete { secret_id: String },
}

/// Response from daemon to secretsd over the authenticated socketpair channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum SecretsdResponse {
    #[serde(rename = "ok")]
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        material: Option<SecretString>,
        #[serde(skip_serializing_if = "Option::is_none")]
        ids: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        existed: Option<bool>,
    },
    #[serde(rename = "error")]
    Error {
        code: SecretsdErrorCode,
        message: String,
    },
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecretsdErrorCode {
    NotFound,
    InvalidRequest,
    Unauthorized,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCode {
    /// Caller is not authorized for this operation.
    Unauthorized,
    /// The requested wrapper or secret does not exist.
    NotFound,
    /// The wrapper/secret is locked and requires unlock first.
    Locked,
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
    /// Read secret material.
    #[serde(rename = "read")]
    Read { secret_id: String },

    /// Write/update secret material.
    #[serde(rename = "write")]
    Write {
        secret_id: String,
        material: SecretString,
    },

    /// List secret IDs under a prefix.
    #[serde(rename = "list")]
    List { prefix: String },

    /// Delete a secret.
    #[serde(rename = "delete")]
    Delete { secret_id: String },

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
        material: Option<SecretString>,
        #[serde(skip_serializing_if = "Option::is_none")]
        info: Option<serde_json::Value>,
    },
    #[serde(rename = "error")]
    Error {
        code: BackendErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackendErrorCode {
    NotFound,
    Internal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_read_request_roundtrip() {
        let req = WrapperRequest::Read {
            scope: Some("session-42".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: WrapperRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            WrapperRequest::Read { scope } => {
                assert_eq!(scope.as_deref(), Some("session-42"));
            }
            other => panic!("expected Read, got {other:?}"),
        }
    }

    #[test]
    fn wrapper_response_ok() {
        let resp = WrapperResponse::Ok {
            env: Some(std::collections::HashMap::from([(
                "TOKEN".to_string(),
                SecretString::from("secret123"),
            )])),
            material: Some(SecretString::from("secret123")),
            message: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"TOKEN\":\"secret123\""));
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
            material: SecretString::from("val"),
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
                assert_eq!(material.as_str(), "val");
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
    fn backend_write_roundtrip() {
        let req = BackendRequest::Write {
            secret_id: "tok".to_string(),
            material: SecretString::from("secret_value"),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"action\":\"write\""));
        // Material should be present in JSON (backend protocol, not user-facing logs).
        assert!(json.contains("\"material\":\"secret_value\""));
    }

    #[test]
    fn backend_list_roundtrip() {
        let req = BackendRequest::List {
            prefix: "gh-authorized/libsecret".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"action\":\"list\""));
        let parsed: BackendRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            BackendRequest::List { prefix } => assert_eq!(prefix, "gh-authorized/libsecret"),
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn backend_delete_roundtrip() {
        let req = BackendRequest::Delete {
            secret_id: "gh-authorized/libsecret/default/item_1".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"action\":\"delete\""));
        let parsed: BackendRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            BackendRequest::Delete { secret_id } => {
                assert_eq!(secret_id, "gh-authorized/libsecret/default/item_1")
            }
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn backend_response_ok_with_material() {
        let resp = BackendResponse::Ok {
            material: Some(SecretString::from("derived_token")),
            info: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"material\":\"derived_token\""));
        assert!(!json.contains("\"info\""));
    }

    #[test]
    fn backend_response_error() {
        let resp = BackendResponse::Error {
            code: BackendErrorCode::NotFound,
            message: "not found".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"error\""));
        assert!(json.contains("\"code\":\"NotFound\""));
    }
}
