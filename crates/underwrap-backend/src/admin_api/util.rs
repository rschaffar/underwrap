//! Cross-cutting helpers shared by admin handlers.
//!
//! These helpers keep shared admin-handler conventions in one place:
//!
//! - Password validation surfaces the canonical
//!   `password_empty` / `password_too_short` error codes.
//! - `saturating_u32` clamps `usize` to `u32` for wire shapes that
//!   declare a `u32` count.
//! - `IntoInternal` turns any `anyhow::Error`-shaped result into a
//!   `500 internal` `AdminError` with a hand-picked context string,
//!   without giving up the discipline of always *picking* one.
//! - The two base64 helpers anchor the admin's binary-payload
//!   encoding on a single base64 engine instance.

use base64::Engine;

use crate::admin_api::AdminError;

/// Reject a password that fails [`crate::auth::validate_password`]
/// with the canonical `password_empty` / `password_too_short` codes.
/// Used by every endpoint that accepts an operator password
/// (create user, reset password, change password, bootstrap).
pub(crate) fn validate_password_or_bad_request(password: &str) -> Result<(), AdminError> {
    if let Err(code) = crate::auth::validate_password(password) {
        return Err(AdminError::bad_request(
            code,
            match code {
                "password_empty" => "password must not be empty",
                "password_too_short" => "password too short",
                _ => "password rejected",
            },
        ));
    }
    Ok(())
}

/// Clamp a `usize` count to fit in the `u32`-typed wire shapes the
/// admin DTOs use for dependent counts.
pub(crate) fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

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

/// Result-adapter trait that lets handlers write
/// `load_x(...).into_internal("loading X document")?` in place of
/// the verbose `.map_err(|e| AdminError::internal("...", e))?`.
///
/// `From<anyhow::Error> for AdminError` is deliberately *not*
/// implemented (see the module note on `admin_api::error`): the
/// context string must be picked explicitly per call site. This
/// trait keeps that discipline while shaving the boilerplate.
pub(crate) trait IntoInternal<T> {
    fn into_internal(self, ctx: &'static str) -> Result<T, AdminError>;
}

impl<T, E: std::fmt::Display> IntoInternal<T> for Result<T, E> {
    fn into_internal(self, ctx: &'static str) -> Result<T, AdminError> {
        self.map_err(|e| AdminError::internal(ctx, e))
    }
}
