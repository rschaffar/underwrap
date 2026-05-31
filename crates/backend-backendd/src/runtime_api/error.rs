//! Unified error type for the runtime mTLS endpoints.
//!
//! Mirrors [`crate::admin_api::error::AdminError`]: handlers return
//! `Result<Json<T>, RuntimeError>`, the `IntoResponse` impl turns the
//! error into the canonical `ServiceErrorResponse` JSON envelope with
//! the right HTTP status, and the wire shape stays identical to what
//! the daemon's `BackendClient` parses.
//!
//! Conversions from `anyhow::Error` are not implemented; handlers
//! pick the context string explicitly via `result.into_internal("...")`.

use std::fmt;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use underwrap_protocol::service::ServiceErrorResponse;

#[derive(Debug)]
pub(crate) struct RuntimeError {
    status: StatusCode,
    code: String,
    message: String,
}

impl RuntimeError {
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

    pub(crate) fn unauthorized(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, code, message)
    }

    pub(crate) fn store_locked(store_id: &str) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            underwrap_protocol::service::ERROR_CODE_STORE_LOCKED,
            format!(
                "store '{store_id}' is locked; an administrator must unlock it before requests can land",
            ),
        )
    }

    pub(crate) fn internal(context: &'static str, error: impl fmt::Display) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            format!("{context}: {error:#}"),
        )
    }
}

impl IntoResponse for RuntimeError {
    fn into_response(self) -> Response {
        let body = ServiceErrorResponse {
            code: self.code,
            message: self.message,
        };
        (self.status, Json(body)).into_response()
    }
}

/// Result-adapter trait so handlers can write
/// `load_x(...).into_internal("loading X document")?` instead of the
/// 7-line `match` block the old hand-built helpers required.
pub(crate) trait IntoInternal<T> {
    fn into_internal(self, context: &'static str) -> Result<T, RuntimeError>;
}

impl<T, E: fmt::Display> IntoInternal<T> for Result<T, E> {
    fn into_internal(self, context: &'static str) -> Result<T, RuntimeError> {
        self.map_err(|e| RuntimeError::internal(context, e))
    }
}
