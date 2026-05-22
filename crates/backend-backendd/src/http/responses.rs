//! Tiny constructors for the `Response<Full<Bytes>>` values every
//! handler returns. Centralising them keeps the per-handler files
//! free of `Response::builder()` boilerplate and makes the JSON
//! envelope (`ServiceErrorResponse`, `application/json`
//! Content-Type) consistent across the surface.

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

/// Parse `Bytes` (collected by Axum's request body extractor) into the
/// caller's request type, returning the canonical `invalid_request`
/// 400 on failure. Axum's built-in `Json` extractor would produce a
/// different error envelope, so we keep ours.
#[allow(clippy::result_large_err)]
pub(crate) fn parse_body<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> std::result::Result<T, Response<Full<Bytes>>> {
    serde_json::from_slice(bytes).map_err(|e| {
        error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            &format!("invalid JSON request body: {e}"),
        )
    })
}

pub(crate) fn json_response<T: serde::Serialize>(
    status: StatusCode,
    body: &T,
) -> Response<Full<Bytes>> {
    let bytes = serde_json::to_vec(body).expect("serializing service response");
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(bytes)))
        .expect("building HTTP response")
}

pub(crate) fn error_response(
    status: StatusCode,
    code: &str,
    message: &str,
) -> Response<Full<Bytes>> {
    json_response(
        status,
        &underwrap_protocol::service::ServiceErrorResponse {
            code: code.to_string(),
            message: message.to_string(),
        },
    )
}

/// Canonical 503 response for a request that targeted a known but
/// locked encrypted store. The `code` field is what frontend daemons
/// and the SPA switch on; the human-readable message names the
/// store so an operator looking at logs can act on it.
pub(crate) fn store_locked_response(store_id: &str) -> Response<Full<Bytes>> {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "store_locked",
        &format!(
            "store '{store_id}' is locked; an administrator must unlock it before requests can land",
        ),
    )
}
