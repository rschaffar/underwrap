//! Serves the operator SPA from the admin listener.
//!
//! The SPA bundle is not a secret, so these routes are public — auth is
//! enforced on the JSON API under `/v1/admin/...` that the SPA then calls.
//!
//! Assets are compiled into the binary via `rust-embed` from
//! `$UNDERWRAP_WEB_DIST`. The Nix flake sets this to the
//! sandbox-built bundle; the workspace-local `build.rs` defaults it
//! to `web/dist/` and **fails the build** when the bundle's
//! `index.html` is missing. A backend binary therefore can never
//! be linked against an empty embed — `serve_spa` takes that
//! invariant for granted.
//!
//! The `debug-embed` feature keeps assets compiled in for every profile.
//! Public requests never read the backend filesystem; rebuilding the SPA
//! therefore also requires rebuilding the backend, including in development.
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
//! See `docs/security.md` "Backend transport and administration" for the threat model behind
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
        None => (
            EmbeddedAssets::get("index.html")
                .expect("build.rs asserts index.html is in the embedded SPA bundle"),
            "index.html".to_string(),
        ),
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

#[cfg(test)]
mod tests;
