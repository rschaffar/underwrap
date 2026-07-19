//! Unified error type for admin handlers.
//!
//! Handlers return `Result<Json<T>, AdminError>`; `IntoResponse` here
//! turns the error into the canonical `ServiceErrorResponse` JSON
//! envelope (`{ code, message }`) with the right HTTP status. The
//! constructor helpers carry the named failure shapes that the admin
//! surface emits over and over (store not found, store locked,
//! internal anyhow wrap with a static context).
//!
//! Conversions from `anyhow::Error` are deliberately *not* implemented:
//! handlers always pick the context string explicitly via
//! `AdminError::internal("loading resources document", err)`.
//!
//! The OpenAPI drift test in `tests/openapi_export.rs` is the
//! authoritative guard on the wire shape (status codes, `code`
//! strings, envelope layout).

use std::fmt;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use underwrap_protocol::service::ServiceErrorResponse;

/// A handler-emitted error. Carries the HTTP status, the canonical
/// `code` string the SPA / frontend daemon switch on, and a human
/// readable message.
#[derive(Debug)]
pub(crate) struct AdminError {
    status: StatusCode,
    code: String,
    message: String,
}

impl AdminError {
    fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }

    pub(crate) fn bad_request(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub(crate) fn store_not_found() -> Self {
        Self::not_found("unknown store")
    }

    pub(crate) fn client_instance_not_found() -> Self {
        Self::not_found("unknown client instance")
    }

    pub(crate) fn resource_not_found() -> Self {
        Self::not_found("unknown resource")
    }

    pub(crate) fn pair_not_found() -> Self {
        Self::not_found("unknown pair")
    }

    pub(crate) fn store_locked(store_id: &str) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "store_locked",
            format!(
                "store '{store_id}' is locked; an administrator must unlock it before requests can land",
            ),
        )
    }

    pub(crate) fn conflict(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    pub(crate) fn unauthorized(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, message)
    }

    pub(crate) fn forbidden(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, code, message)
    }

    pub(crate) fn too_many_requests(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, code, message)
    }

    /// `500 internal` with a static context string prefixed to the
    /// error formatted via `{:#}`.
    pub(crate) fn internal(context: &'static str, error: impl fmt::Display) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("{context}: {error:#}"),
        )
    }

    /// `500` with a hand-picked code (e.g. `list_store_failed`,
    /// `write_client_instances_failed`). Used where the wire contract
    /// surfaces a more specific 500 code than the generic `internal`.
    pub(crate) fn internal_with_code(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, code, message)
    }

    pub(crate) fn rotation_recovery_required() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "rotation_recovery_required",
            "the canonical encrypted generation is unavailable; stop the backend and restore a retained generation using the documented trusted-host recovery procedure",
        )
    }

    #[cfg(test)]
    pub(crate) fn status(&self) -> StatusCode {
        self.status
    }
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let body = ServiceErrorResponse {
            code: self.code,
            message: self.message,
        };
        (self.status, Json(body)).into_response()
    }
}
