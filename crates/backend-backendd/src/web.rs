//! Serves the operator SPA from the admin listener.
//!
//! The SPA bundle is not a secret, so these routes are public — auth is
//! enforced on the JSON API under `/v1/admin/...` that the SPA then calls.
//!
//! Assets are compiled into the binary via `rust-embed` from
//! `$UNDERWRAP_WEB_DIST` (the Nix flake points this at the built `web/dist/`).
//! In debug builds, `rust-embed` reads the same files dynamically from disk
//! at request time, so a `npm run build` is enough to iterate on the SPA
//! without rebuilding the daemon. Release builds bake the assets in.
//!
//! For any non-API GET/HEAD that does not match a file, [`serve_spa`]
//! falls back to `index.html` so client-side routing can take over.
//! Path-traversal is harmless: `rust_embed::get` is a table lookup over
//! a precomputed set of asset keys, so `../etc/passwd` simply misses
//! and falls back like any other unknown route.
//!
//! `Cache-Control` is set per resolved asset:
//!
//! * `index.html` → `no-store` so a stale shell never references
//!   deleted hashed assets after a deploy.
//! * `assets/*` → `public, max-age=31536000, immutable` because Vite
//!   emits content-hashed filenames there.
//! * Everything else → a short, conservative `must-revalidate` cache.
//!
//! Wire this in by attaching [`serve_spa`] as the top-level
//! `Router::fallback`.
//!
//! See `docs/security.md` "Admin listener" for the threat model behind
//! shipping the SPA inside the binary closure.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode, header};
use axum::response::Response;
use rust_embed::RustEmbed;

#[derive(RustEmbed, Clone)]
#[folder = "$UNDERWRAP_WEB_DIST"]
struct EmbeddedAssets;

/// Axum fallback handler that serves the SPA bundle for any GET/HEAD
/// that no other route matched. The caller is expected to dispatch
/// non-GET/HEAD methods to the canonical 404 envelope itself —
/// `serve_spa` only handles SPA-shaped requests and panics on other
/// methods to make routing mistakes loud in debug.
pub async fn serve_spa(req: Request) -> Response {
    debug_assert!(matches!(*req.method(), Method::GET | Method::HEAD));

    let raw = req.uri().path().trim_start_matches('/');
    let lookup = if raw.is_empty() { "index.html" } else { raw };

    let (asset, resolved) = match EmbeddedAssets::get(lookup) {
        Some(file) => (file, lookup.to_string()),
        None => match EmbeddedAssets::get("index.html") {
            Some(file) => (file, "index.html".to_string()),
            None => return spa_unavailable(),
        },
    };

    let body = if *req.method() == Method::HEAD {
        Body::empty()
    } else {
        Body::from(asset.data.into_owned())
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type_for(&resolved))
        .header(header::CACHE_CONTROL, cache_control_for(&resolved))
        .body(body)
        .expect("response builds from valid headers and body")
}

fn content_type_for(rel: &str) -> String {
    mime_guess::from_path(rel)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

fn cache_control_for(rel: &str) -> &'static str {
    if rel == "index.html" {
        "no-store"
    } else if rel.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=300, must-revalidate"
    }
}

fn spa_unavailable() -> Response {
    let body = "<!doctype html>\
        <html><head><meta charset=\"utf-8\"><title>underwrap admin</title></head>\
        <body style=\"font-family:sans-serif;padding:2em;\">\
        <h1>SPA not built</h1>\
        <p>The admin UI bundle is not embedded. Build <code>web/</code> and \
        rebuild the backend (the Nix flake's <code>underwrap-backendd</code> \
        derivation does this automatically).</p>\
        </body></html>";
    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .expect("static body builds")
}

#[cfg(test)]
mod tests;
