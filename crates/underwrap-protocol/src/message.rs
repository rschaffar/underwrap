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
    Error {
        code: ErrorCode,
        message: String,
    },
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
        material: String,
    },
    // Future: Unlock, Lock, Reauth, Delete, ...
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
    Error {
        code: ErrorCode,
        message: String,
    },
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
