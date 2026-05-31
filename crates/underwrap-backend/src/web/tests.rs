use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode, header};
use http_body_util::BodyExt;

use super::serve_spa;

fn get(path: &str) -> Request {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

fn head(path: &str) -> Request {
    Request::builder()
        .method(Method::HEAD)
        .uri(path)
        .body(Body::empty())
        .unwrap()
}

/// Embedded SPA smoke test. `build.rs` already refuses to compile if
/// `index.html` is missing, so this is a defense-in-depth check that
/// also catches a rust-embed misconfiguration (e.g. the macro
/// resolved the env var to a different path than `build.rs` did).
#[tokio::test]
async fn root_serves_index_html_no_store() {
    let resp = serve_spa(get("/")).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "embedded SPA bundle is empty — was web/dist/ built before this test ran?"
    );
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/html"
    );
    assert_eq!(
        resp.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let text = std::str::from_utf8(&body).expect("index.html is valid UTF-8");
    assert!(
        text.contains("<title>underwrap</title>"),
        "embedded index.html missing expected title marker"
    );
}

#[tokio::test]
async fn spa_history_route_falls_back_to_index() {
    let resp = serve_spa(get("/users/123/edit")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/html"
    );
    assert_eq!(
        resp.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
}

#[tokio::test]
async fn missing_path_under_assets_also_falls_back_to_index() {
    // The SPA owns routing; missing /assets/foo.js paths fall back to
    // index.html with the no-store cache rule (because the resolved
    // file is index.html, not the requested asset).
    let resp = serve_spa(get("/assets/definitely-missing.js")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/html"
    );
    assert_eq!(
        resp.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
}

#[tokio::test]
async fn path_traversal_request_falls_back_to_index_and_never_escapes() {
    // `EmbeddedAssets::get` is a table lookup; `../etc/passwd` is not
    // a key in the embed, so it misses and falls back to index.html.
    let resp = serve_spa(get("/../../../etc/passwd")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/html"
    );
}

#[tokio::test]
async fn hashed_asset_is_served_immutable() {
    // Walk the embedded set for the first /assets/ entry. Vite emits
    // `assets/<name>-<hash>.<ext>`, so there is always at least one.
    let asset = <super::EmbeddedAssets as rust_embed::RustEmbed>::iter()
        .find(|p| p.starts_with("assets/"))
        .expect("at least one hashed asset in the SPA bundle");

    let resp = serve_spa(get(&format!("/{asset}"))).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CACHE_CONTROL).unwrap(),
        "public, max-age=31536000, immutable"
    );
}

#[tokio::test]
async fn head_request_returns_empty_body_with_headers() {
    let resp = serve_spa(head("/")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/html"
    );
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(body.is_empty(), "HEAD body must be empty");
}
