//! Serves the operator SPA from the admin listener.
//!
//! The SPA bundle is not a secret, so these routes are public — auth is
//! enforced on the JSON API under `/v1/admin/...` that the SPA then calls.
//!
//! Two sources are supported:
//!
//! * **Disk**: assets read from a directory pointed at by
//!   `UNDERWRAP_BACKENDD_WEB_DIR` at startup. Used for development so the
//!   SPA can be rebuilt without rebuilding the Rust binary.
//! * **Embedded**: assets compiled into the binary via `rust-embed` from
//!   `web/dist/`. Used in the release build produced by the Nix flake.
//!
//! For any non-API GET that does not match a file, the handler falls
//! back to `index.html` so client-side routing can take over.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::{Method, Response, StatusCode};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "$UNDERWRAP_WEB_DIST"]
struct EmbeddedAssets;

/// Where to find the SPA bundle. Cheap to clone; the disk path is held
/// in an `Arc` so this can be cloned per request without extra allocs.
#[derive(Clone, Debug)]
pub enum WebAssetSource {
    /// Read assets from a directory on disk. The dev path.
    Disk(Arc<PathBuf>),
    /// Read assets from the binary's embedded asset store. The release path.
    Embedded,
}

impl WebAssetSource {
    /// Resolve the source from `UNDERWRAP_BACKENDD_WEB_DIR`. Falls back to
    /// the embedded source when the env var is unset.
    pub fn from_env() -> Self {
        match std::env::var("UNDERWRAP_BACKENDD_WEB_DIR") {
            Ok(path) if !path.trim().is_empty() => Self::Disk(Arc::new(PathBuf::from(path.trim()))),
            _ => Self::Embedded,
        }
    }
}

/// Result of an asset lookup before we wrap it into an HTTP response.
struct LoadedAsset {
    body: Vec<u8>,
    /// Relative path inside the asset store, used for MIME and cache
    /// header decisions. Always normalised (no leading slash, forward
    /// slashes only).
    rel_path: String,
}

/// Try to load `rel` from the configured source. Returns `None` when
/// the file is not present.
fn load(source: &WebAssetSource, rel: &str) -> Option<LoadedAsset> {
    let normalised = normalise_rel_path(rel);
    let lookup = if normalised.is_empty() {
        "index.html".to_string()
    } else {
        normalised
    };
    match source {
        WebAssetSource::Disk(root) => {
            let candidate = root.join(&lookup);
            if !is_inside(root.as_ref(), &candidate) {
                return None;
            }
            std::fs::read(&candidate).ok().map(|body| LoadedAsset {
                body,
                rel_path: lookup,
            })
        }
        WebAssetSource::Embedded => EmbeddedAssets::get(&lookup).map(|file| LoadedAsset {
            body: file.data.into_owned(),
            rel_path: lookup,
        }),
    }
}

/// Normalise a request path to a relative asset path:
/// strips the leading slash, rejects path-traversal segments, and
/// folds away `./` segments. Returns an empty string for `/`, signalling
/// "serve `index.html`".
fn normalise_rel_path(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    // Reject anything containing `..` segments so an attacker can't escape
    // the asset root via either source.
    if trimmed
        .split('/')
        .any(|segment| segment == ".." || segment == ".")
    {
        return String::from("__rejected__");
    }
    trimmed.to_string()
}

/// Defence in depth: in disk mode, reject any candidate that resolves
/// outside the configured root. `path` is allowed to not exist; we
/// compare component prefixes so symlinks aren't followed.
fn is_inside(root: &Path, candidate: &Path) -> bool {
    let root_components: Vec<_> = root.components().collect();
    let candidate_components: Vec<_> = candidate.components().collect();
    if candidate_components.len() < root_components.len() {
        return false;
    }
    candidate_components
        .iter()
        .zip(root_components.iter())
        .all(|(a, b)| a == b)
}

/// True for paths that look like a content-hashed build artifact:
/// `/assets/<name>-<hash>.<ext>`. Vite (and Rspack and friends) emit
/// hashed filenames into `assets/`, so we mark those as immutable.
fn is_hashed_asset(rel_path: &str) -> bool {
    rel_path.starts_with("assets/")
}

fn content_type_for(rel_path: &str) -> String {
    mime_guess::from_path(rel_path)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

fn cache_control_for(rel_path: &str) -> &'static str {
    if rel_path == "index.html" {
        // The shell that boots the SPA must always be fresh, otherwise a
        // stale index.html will reference assets that no longer exist
        // post-deploy.
        "no-store"
    } else if is_hashed_asset(rel_path) {
        "public, max-age=31536000, immutable"
    } else {
        // Conservative default for anything else (e.g. /favicon.ico,
        // /robots.txt) — short cache, must revalidate.
        "public, max-age=300, must-revalidate"
    }
}

/// True if a request should be served by the SPA handler. The admin
/// JSON API lives under `/v1/...`; any GET/HEAD outside that prefix is
/// a candidate for the SPA.
pub fn is_spa_request(method: &Method, path: &str) -> bool {
    if !matches!(method, &Method::GET | &Method::HEAD) {
        return false;
    }
    !path.starts_with("/v1/")
}

/// Serve a GET/HEAD request against the SPA bundle. Performs history
/// fallback to `index.html` when the requested path doesn't resolve
/// to a file.
pub fn serve_web_request(
    method: &Method,
    path: &str,
    source: &WebAssetSource,
) -> Response<Full<Bytes>> {
    debug_assert!(is_spa_request(method, path));

    let loaded = match load(source, path) {
        Some(asset) => asset,
        None => match load(source, "index.html") {
            Some(asset) => asset,
            None => return spa_not_available_response(),
        },
    };

    let body = if method == Method::HEAD {
        Full::new(Bytes::new())
    } else {
        Full::new(Bytes::from(loaded.body))
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(
            hyper::header::CONTENT_TYPE,
            content_type_for(&loaded.rel_path),
        )
        .header(
            hyper::header::CACHE_CONTROL,
            cache_control_for(&loaded.rel_path),
        )
        .body(body)
        .expect("response builds from valid headers and body")
}

fn spa_not_available_response() -> Response<Full<Bytes>> {
    let body = "<!doctype html>\
        <html><head><meta charset=\"utf-8\"><title>underwrap admin</title></head>\
        <body style=\"font-family:sans-serif;padding:2em;\">\
        <h1>SPA not built</h1>\
        <p>The admin UI bundle is not embedded and \
        <code>UNDERWRAP_BACKENDD_WEB_DIR</code> is not set. \
        Build <code>web/</code> or set the env var to a \
        <code>dist/</code> directory.</p>\
        </body></html>";
    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header(hyper::header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(hyper::header::CACHE_CONTROL, "no-store")
        .body(Full::new(Bytes::from_static(body.as_bytes())))
        .expect("static body builds")
}

#[cfg(test)]
mod tests;
