//! Unified error type for admin handlers.
//!
//! Handlers return `Result<Json<T>, AdminError>`; `IntoResponse` here
//! turns the error into the canonical `ServiceErrorResponse` JSON
//! envelope (`{ code, message }`) with the right HTTP status. The
//! constructor helpers carry the named failure shapes that the admin
//! surface emits over and over (store not found, store locked, internal
//! anyhow wrap with a static context).
//!
//! Conversions from `anyhow::Error` are deliberately *not* implemented:
//! handlers always pick the context string explicitly via
//! `AdminError::internal("loading resources document", err)`, which
//! mirrors the discipline the old hand-rolled `error_response(...,
//! &format!("loading X document: {e:#}"))` sites enforced.
//!
//! The wire shape (`ServiceErrorResponse`, status codes, `code`
//! strings) must stay byte-identical to what the legacy
//! `error_response` / `store_locked_response` helpers produced — the
//! OpenAPI drift test in `tests/openapi_export.rs` enforces this.

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
    pub(crate) fn new(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
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
    /// error formatted via `{:#}`. Mirrors the legacy
    /// `error_response(500, "internal", &format!("loading X: {e:#}"))`
    /// shape so the wire message stays identical.
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

    /// `400 admin_error` envelope — the legacy catch-all for `anyhow`
    /// errors that surface to admin clients as a bad request without
    /// finer classification. Kept as a named constructor so the wire
    /// `code` is identical to what the old `error_response(400,
    /// "admin_error", &e.to_string())` sites produced.
    pub(crate) fn admin(error: impl fmt::Display) -> Self {
        Self::bad_request("admin_error", error.to_string())
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
