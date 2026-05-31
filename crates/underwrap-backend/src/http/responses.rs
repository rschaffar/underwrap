//! Shared HTTP-layer helpers for the backend listener.
//!
//! Exports [`Body`], the JSON-request extractor every POST handler
//! uses. Axum's built-in `Json<T>` extractor rejects with its own
//! error shape; ours preserves the canonical
//! `{ "code": "invalid_request", "message": ... }` envelope by
//! delegating to [`crate::admin_api::AdminError`].
//!
//! Runtime-side handlers that still return raw
//! `Response<Full<Bytes>>` use the helpers in
//! [`crate::runtime_api::responses`].

use axum::body::to_bytes;
use axum::extract::{FromRequest, Request};
use axum::response::IntoResponse;

use crate::admin_api::AdminError;

/// Maximum admin/runtime request body size. The largest payloads we
/// expect are policy documents and resource blobs in the low hundreds
/// of KB; 2 MiB leaves plenty of headroom while still bounding memory.
pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 2 * 1024 * 1024;

/// JSON request extractor. Reads the body up to
/// [`MAX_REQUEST_BODY_BYTES`], parses it as `T`, and rejects with the
/// canonical `invalid_request` 400 envelope on either failure.
pub(crate) struct Body<T>(pub(crate) T);

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
                AdminError::bad_request(
                    "invalid_request",
                    format!("failed to read request body: {e}"),
                )
                .into_response()
            })?;
        serde_json::from_slice(&bytes).map(Body).map_err(|e| {
            AdminError::bad_request("invalid_request", format!("invalid JSON request body: {e}"))
                .into_response()
        })
    }
}
