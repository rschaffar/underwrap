//! Base64 helpers shared by admin handlers.
//!
//! Admin endpoints exchange binary payloads (secret material, item
//! bytes) as base64 strings inside JSON bodies. Every handler used to
//! reach for `base64::engine::general_purpose::STANDARD` and map the
//! decode error into the canonical `invalid_request` 400 envelope by
//! hand. These helpers keep the engine choice and the error shape in
//! one place.

use base64::Engine;

use crate::admin_api::AdminError;

/// Decode a base64 string from a request body, surfacing a 400
/// `invalid_request` on malformed input.
pub(crate) fn decode_b64(input: &str) -> Result<Vec<u8>, AdminError> {
    base64::engine::general_purpose::STANDARD
        .decode(input)
        .map_err(|e| {
            AdminError::bad_request("invalid_request", format!("invalid base64 payload: {e}"))
        })
}

/// Encode bytes to a base64 string for a response body.
pub(crate) fn encode_b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
