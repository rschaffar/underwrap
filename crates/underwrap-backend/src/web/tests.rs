use axum::body::Body;
use axum::extract::Request;
use axum::http::{Method, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::{EmbeddedAssets, serve_spa};

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
    // Unknown asset keys must fall back without reading the host filesystem.
    let resp = serve_spa(get("/../../../etc/passwd")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/html"
    );
}

/// Reproduce RF-0002 through the real fallback without touching the bundle
/// or any private file. The outside final-component symlink is created only
/// after compilation, so it cannot be an intentionally embedded asset.
#[tokio::test]
async fn outside_symlink_request_returns_only_embedded_index() {
    let tmp = tempfile::TempDir::new().unwrap();
    let outside = tmp.path().join("outside.txt");
    let link = tmp.path().join("outside-link");
    std::fs::write(&outside, b"PUBLIC RF-0002 TEST MARKER").unwrap();
    std::os::unix::fs::symlink(&outside, &link).unwrap();

    let root = std::path::Path::new(env!("UNDERWRAP_WEB_DIST"))
        .canonicalize()
        .unwrap();
    let parents = "../".repeat(root.components().count());
    let outside_path =
        |path: &std::path::Path| format!("/{parents}{}", path.strip_prefix("/").unwrap().display());
    let link_uri = outside_path(&link);
    assert!(get(&link_uri).uri().path().contains("/../"));
    assert!(!link.canonicalize().unwrap().starts_with(&root));
    assert!(!EmbeddedAssets::iter().any(|key| key == link_uri.trim_start_matches('/')));

    let index = EmbeddedAssets::get("index.html").unwrap();
    let app = axum::Router::new().fallback(serve_spa);
    // Keep ordinary external-file traversal as an independent negative control,
    // and valid shell/history requests as positive controls. A 200 alone is not
    // confinement: both the leaked file and legitimate history fallback use it.
    for path in [
        outside_path(&outside),
        link_uri.clone(),
        "/".into(),
        "/index.html".into(),
        "/history/route".into(),
    ] {
        let resp = app.clone().oneshot(get(&path)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(
            body.as_ref() == index.data.as_ref(),
            "expected only SPA index"
        );
    }

    let resp = app.oneshot(head(&link_uri)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "text/html");
    assert_eq!(resp.headers()[header::CACHE_CONTROL], "no-store");
    assert!(
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
}

#[test]
fn assets_are_embedded_even_with_debug_assertions() {
    for key in EmbeddedAssets::iter() {
        let asset = EmbeddedAssets::get(&key).unwrap();
        assert!(
            matches!(asset.data, std::borrow::Cow::Borrowed(_)),
            "SPA assets must be compiled in, not read from disk at request time"
        );
    }
}

#[tokio::test]
async fn hashed_asset_is_served_immutable() {
    // Walk the embedded set for the first /assets/ entry. Vite emits
    // `assets/<name>-<hash>.<ext>`, so there is always at least one.
    let asset = EmbeddedAssets::iter()
        .find(|p| p.starts_with("assets/"))
        .expect("at least one hashed asset in the SPA bundle");

    let resp = serve_spa(get(&format!("/{asset}"))).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CACHE_CONTROL).unwrap(),
        "public, max-age=31536000, immutable"
    );
    assert_eq!(
        resp.headers()[header::CONTENT_TYPE],
        super::content_type_for(&asset)
    );
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(body.as_ref() == EmbeddedAssets::get(&asset).unwrap().data.as_ref());

    let resp = serve_spa(head(&format!("/{asset}"))).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()[header::CONTENT_TYPE],
        super::content_type_for(&asset)
    );
    assert_eq!(
        resp.headers()[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    assert!(
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
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
