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
    // matches Vite / CRA conventions.
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

/// Smoke test for the embedded SPA bundle. Fails when the embed is
/// empty / placeholder-only — i.e. when `$UNDERWRAP_WEB_DIST` at
/// compile time pointed at an unbuilt `web/dist/`. Without this
/// gate the binary builds and starts fine but serves 503 to every
/// `/` request.
///
/// **Local development**: this test requires the SPA to be built.
/// Run `cd web && npm run build` (or `nix build .#underwrap-web-dist`)
/// before `cargo test -p backend-backendd`. The Nix derivation sets
/// `UNDERWRAP_WEB_DIST` to the built bundle automatically.
#[tokio::test]
async fn embedded_assets_contain_built_spa_index() {
    use http_body_util::BodyExt;
    let resp = serve_web_request(&Method::GET, "/", &WebAssetSource::Embedded);
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "embedded SPA bundle is empty — was web/dist/ built before this test ran? \
         See the test's doc comment for the local-dev procedure."
    );
    assert_eq!(
        resp.headers().get(hyper::header::CONTENT_TYPE).unwrap(),
        "text/html"
    );
    // `index.html`'s `<title>underwrap</title>` is the most stable
    // marker we can assert against — the surrounding head / body
    // varies with each Vite build (asset hashes change).
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let body_str = std::str::from_utf8(&body).expect("index.html is valid UTF-8");
    assert!(
        body_str.contains("<title>underwrap</title>"),
        "embedded index.html does not contain the expected title marker"
    );
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
