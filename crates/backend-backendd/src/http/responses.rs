//! Tiny constructors for the `Response<Full<Bytes>>` values every
//! handler returns. Centralising them keeps the per-handler files
//! free of `Response::builder()` boilerplate and makes the JSON
//! envelope (`ServiceErrorResponse`, `application/json`
//! Content-Type) consistent across the surface.
//!
//! Also exports [`Body`], the JSON-request extractor every POST handler
//! uses. Axum's built-in `Json<T>` extractor rejects with its own
//! error shape; ours preserves the canonical
//! `{ "code": "invalid_request", "message": ... }` envelope.

use axum::body::to_bytes;
use axum::extract::{FromRequest, Request};
use axum::response::IntoResponse;
use bytes::Bytes;
use http_body_util::Full;
use hyper::{Response, StatusCode};

/// Maximum admin/runtime request body size. The largest payloads we
/// expect are policy documents and resource blobs in the low hundreds
/// of KB; 2 MiB leaves plenty of headroom while still bounding memory.
const MAX_REQUEST_BODY_BYTES: usize = 2 * 1024 * 1024;

/// JSON request extractor. Reads the body up to
/// [`MAX_REQUEST_BODY_BYTES`], parses it as `T`, and rejects with the
/// canonical `invalid_request` 400 on either failure.
pub(crate) struct Body<T>(pub T);

impl<S, T> FromRequest<S> for Body<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = axum::response::Response;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let bytes = to_bytes(req.into_body(), MAX_REQUEST_BODY_BYTES)
            .await
            .map_err(|e| {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    &format!("failed to read request body: {e}"),
                )
                .into_response()
            })?;
        serde_json::from_slice(&bytes).map(Body).map_err(|e| {
            error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &format!("invalid JSON request body: {e}"),
            )
            .into_response()
        })
    }
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
