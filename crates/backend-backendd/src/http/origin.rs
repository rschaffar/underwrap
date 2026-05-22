//! `Origin` / `Referer` allowlisting for state-changing admin
//! requests. Read in conjunction with the CSRF double-submit check in
//! `admin_api::csrf`; the two together form the cross-site request
//! defence.
//!
//! [`validate_admin_config`] runs at startup to reject obviously
//! malformed allowlist entries; [`check_admin_origin`] runs on every
//! state-changing admin request once the daemon is live.

use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use hyper::{Method, StatusCode};
use tracing::warn;

use crate::config::AdminConfig;
use crate::http::responses::error_response;
use crate::runtime_state::AdminRuntimeState;

/// Validate the browser-facing admin configuration.
pub(crate) fn validate_admin_config(config: &AdminConfig) -> Result<()> {
    if let Some(origins) = &config.allowed_origins {
        for origin in origins {
            if origin.trim().is_empty() {
                anyhow::bail!("admin.allowed_origins contains an empty entry");
            }
        }
    }
    Ok(())
}

/// Check the `Origin` (or `Referer`) header of a state-changing request
/// against the configured allowlist. GET/HEAD/OPTIONS are not gated.
/// Returns `Ok(())` when no allowlist is configured.
pub(crate) fn check_admin_origin(
    method: &Method,
    headers: &hyper::HeaderMap,
    allowed: Option<&[String]>,
) -> std::result::Result<(), &'static str> {
    if matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS) {
        return Ok(());
    }
    let allowed = match allowed {
        Some(list) if !list.is_empty() => list,
        _ => return Ok(()),
    };
    let presented = headers
        .get(hyper::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get(hyper::header::REFERER)
                .and_then(|v| v.to_str().ok())
                .and_then(referer_to_origin)
        });
    let presented = match presented {
        Some(value) => value,
        None => return Err("missing Origin/Referer on state-changing request"),
    };
    if allowed.iter().any(|a| a.trim() == presented) {
        Ok(())
    } else {
        Err("Origin/Referer not in allowed_origins")
    }
}

/// Strip path/query from a `Referer` value, returning the scheme+host[:port]
/// prefix. Returns `None` for malformed inputs so callers can reject.
pub(crate) fn referer_to_origin(referer: &str) -> Option<String> {
    let trimmed = referer.trim();
    let (scheme, rest) = trimmed.split_once("://")?;
    if scheme.is_empty() {
        return None;
    }
    let authority = match rest.find('/') {
        Some(idx) => &rest[..idx],
        None => rest,
    };
    if authority.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{authority}"))
}

/// Axum middleware that runs the origin check before the inner service.
/// Short-circuits with `403 bad_origin` on rejection.
pub(crate) async fn origin_layer(
    State(state): State<Arc<AdminRuntimeState>>,
    request: Request,
    next: Next,
) -> Response {
    use axum::response::IntoResponse;
    if let Err(reason) = check_admin_origin(
        request.method(),
        request.headers(),
        state.allowed_origins.as_deref(),
    ) {
        warn!(reason = reason, "rejected admin request (origin check)");
        return error_response(StatusCode::FORBIDDEN, "bad_origin", reason).into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests;
