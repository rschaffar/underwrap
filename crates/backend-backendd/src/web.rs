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
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist/"]
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
        Build <code>web/</code> (ticket 220) or set the env var to a \
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
mod tests {
    use super::*;

    #[test]
    fn normalise_rel_path_handles_root_and_traversal() {
        assert_eq!(normalise_rel_path("/"), "");
        assert_eq!(normalise_rel_path(""), "");
        assert_eq!(normalise_rel_path("/index.html"), "index.html");
        assert_eq!(normalise_rel_path("/assets/foo.js"), "assets/foo.js");
        assert_eq!(normalise_rel_path("/../etc/passwd"), "__rejected__");
        assert_eq!(normalise_rel_path("/assets/../secret"), "__rejected__");
        assert_eq!(normalise_rel_path("/assets/./x"), "__rejected__");
    }

    #[test]
    fn is_hashed_asset_only_matches_assets_dir() {
        assert!(is_hashed_asset("assets/index-AbC123.js"));
        assert!(is_hashed_asset("assets/style-9f.css"));
        assert!(!is_hashed_asset("index.html"));
        assert!(!is_hashed_asset("favicon.ico"));
        assert!(!is_hashed_asset("robots.txt"));
    }

    #[test]
    fn content_type_for_common_extensions() {
        assert_eq!(content_type_for("index.html"), "text/html");
        assert_eq!(content_type_for("assets/x.js"), "text/javascript");
        assert_eq!(content_type_for("assets/x.css"), "text/css");
        assert_eq!(content_type_for("assets/font.woff2"), "font/woff2");
        assert_eq!(content_type_for("logo.svg"), "image/svg+xml");
        assert_eq!(content_type_for("data.bin"), "application/octet-stream");
    }

    #[test]
    fn cache_control_distinguishes_index_assets_and_other() {
        assert_eq!(cache_control_for("index.html"), "no-store");
        assert_eq!(
            cache_control_for("assets/index-9f.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            cache_control_for("favicon.ico"),
            "public, max-age=300, must-revalidate"
        );
    }

    #[test]
    fn is_spa_request_routes_correctly() {
        assert!(is_spa_request(&Method::GET, "/"));
        assert!(is_spa_request(&Method::GET, "/assets/x.js"));
        assert!(is_spa_request(&Method::HEAD, "/index.html"));
        assert!(!is_spa_request(&Method::GET, "/v1/admin/users"));
        assert!(!is_spa_request(&Method::POST, "/"));
        assert!(!is_spa_request(&Method::POST, "/assets/x.js"));
        assert!(!is_spa_request(&Method::DELETE, "/"));
    }

    fn fixture_source() -> (tempfile::TempDir, WebAssetSource) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("index.html"),
            "<!doctype html><body>app</body>",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("assets")).unwrap();
        std::fs::write(dir.path().join("assets/index-9f.js"), "console.log('ok');").unwrap();
        let source = WebAssetSource::Disk(Arc::new(dir.path().to_path_buf()));
        (dir, source)
    }

    #[test]
    fn serve_web_request_returns_index_for_root() {
        let (_dir, source) = fixture_source();
        let resp = serve_web_request(&Method::GET, "/", &source);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(hyper::header::CONTENT_TYPE).unwrap(),
            "text/html"
        );
        assert_eq!(
            resp.headers().get(hyper::header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
    }

    #[test]
    fn serve_web_request_returns_hashed_asset_with_immutable_cache() {
        let (_dir, source) = fixture_source();
        let resp = serve_web_request(&Method::GET, "/assets/index-9f.js", &source);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(hyper::header::CONTENT_TYPE).unwrap(),
            "text/javascript"
        );
        assert_eq!(
            resp.headers().get(hyper::header::CACHE_CONTROL).unwrap(),
            "public, max-age=31536000, immutable"
        );
    }

    #[test]
    fn serve_web_request_history_fallback_returns_index_for_unknown_paths() {
        let (_dir, source) = fixture_source();
        let resp = serve_web_request(&Method::GET, "/users/123/edit", &source);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(hyper::header::CONTENT_TYPE).unwrap(),
            "text/html"
        );
    }

    #[test]
    fn serve_web_request_history_fallback_does_not_apply_to_missing_assets() {
        // An unknown path under /assets/ also falls back to index.html
        // because the SPA owns the routing decisions; the SPA itself
        // will surface a real 404 inside the page if needed. This
        // matches Vite / CRA conventions and is what the ticket
        // ("any non-API GET that does not match a file") prescribes.
        let (_dir, source) = fixture_source();
        let resp = serve_web_request(&Method::GET, "/assets/missing.js", &source);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(hyper::header::CONTENT_TYPE).unwrap(),
            "text/html"
        );
    }

    #[test]
    fn serve_web_request_returns_placeholder_when_no_index() {
        let dir = tempfile::TempDir::new().unwrap();
        // No index.html, no assets.
        let source = WebAssetSource::Disk(Arc::new(dir.path().to_path_buf()));
        let resp = serve_web_request(&Method::GET, "/", &source);
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            resp.headers().get(hyper::header::CACHE_CONTROL).unwrap(),
            "no-store"
        );
    }

    #[test]
    fn serve_web_request_rejects_path_traversal_with_fallback() {
        let (_dir, source) = fixture_source();
        // Traversal segments are normalised to a rejected marker, so the
        // lookup fails and the handler serves index.html (history
        // fallback). It must never read outside the asset root.
        let resp = serve_web_request(&Method::GET, "/../../../etc/passwd", &source);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(hyper::header::CONTENT_TYPE).unwrap(),
            "text/html"
        );
    }

    #[tokio::test]
    async fn serve_web_request_head_returns_empty_body() {
        use http_body_util::BodyExt;
        let (_dir, source) = fixture_source();
        let resp = serve_web_request(&Method::HEAD, "/index.html", &source);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(hyper::header::CONTENT_TYPE).unwrap(),
            "text/html"
        );
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty(), "HEAD body must be empty");
    }

    #[tokio::test]
    async fn serve_web_request_get_returns_body() {
        use http_body_util::BodyExt;
        let (_dir, source) = fixture_source();
        let resp = serve_web_request(&Method::GET, "/index.html", &source);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(!body.is_empty());
        assert!(body.starts_with(b"<!doctype html>"));
    }

    #[test]
    fn from_env_uses_disk_when_var_set() {
        // Use a guard to restore env var state.
        let prev = std::env::var("UNDERWRAP_BACKENDD_WEB_DIR").ok();
        unsafe {
            std::env::set_var("UNDERWRAP_BACKENDD_WEB_DIR", "/tmp/some/spa");
        }
        let s = WebAssetSource::from_env();
        match s {
            WebAssetSource::Disk(p) => assert_eq!(p.as_path(), Path::new("/tmp/some/spa")),
            _ => panic!("expected Disk source"),
        }
        unsafe {
            match prev {
                Some(v) => std::env::set_var("UNDERWRAP_BACKENDD_WEB_DIR", v),
                None => std::env::remove_var("UNDERWRAP_BACKENDD_WEB_DIR"),
            }
        }
    }

    #[test]
    fn from_env_uses_embedded_when_var_empty() {
        let prev = std::env::var("UNDERWRAP_BACKENDD_WEB_DIR").ok();
        unsafe {
            std::env::set_var("UNDERWRAP_BACKENDD_WEB_DIR", "   ");
        }
        let s = WebAssetSource::from_env();
        assert!(matches!(s, WebAssetSource::Embedded));
        unsafe {
            match prev {
                Some(v) => std::env::set_var("UNDERWRAP_BACKENDD_WEB_DIR", v),
                None => std::env::remove_var("UNDERWRAP_BACKENDD_WEB_DIR"),
            }
        }
    }
}
