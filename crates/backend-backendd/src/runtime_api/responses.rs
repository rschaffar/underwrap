//! Response constructors for the runtime mTLS endpoints.
//!
//! Runtime handlers return raw `Response<Full<Bytes>>` rather than
//! going through axum's `Json<T>` extractor; the helpers below
//! produce the same `ServiceErrorResponse` envelope the admin side
//! emits via [`crate::admin_api::AdminError`], so the wire shape
//! stays uniform across both APIs.

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

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
/// switch on; the human-readable message names the store so an
/// operator looking at logs can act on it.
pub(crate) fn store_locked_response(store_id: &str) -> Response<Full<Bytes>> {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "store_locked",
        &format!(
            "store '{store_id}' is locked; an administrator must unlock it before requests can land",
        ),
    )
}
