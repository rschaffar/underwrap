//! HTTP-layer plumbing for the backend listener.
//!
//! Submodules:
//!
//! - [`responses`] — small helpers that every handler uses to emit
//!   `Response<Full<Bytes>>` values (`json_response`,
//!   `error_response`, `store_locked_response`, `parse_body`).
//! - [`origin`] — `Origin` / `Referer` allowlisting on state-changing
//!   requests + the matching configuration-validation helper, plus
//!   the Axum origin middleware layer.
//!
//! This module also owns the TLS accept loop:
//! [`https_accept_loop`] composes the admin and runtime [`axum::Router`]s
//! into one service, accepts TLS connections, injects each connection's
//! peer-certificate chain as a request extension, and serves the
//! per-connection `tower::Service` through `hyper_util`.

use anyhow::{Context, Result};
use axum::Extension;
use axum::Router;
use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response as AxumResponse};
use hyper::StatusCode;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as ServerBuilder;
use hyper_util::service::TowerToHyperService;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::error;

use crate::admin_api::admin_router;
use crate::http::responses::error_response;
use crate::runtime_api::{PeerCertificates, runtime_router};
use crate::runtime_state::AdminRuntimeState;

pub(crate) mod origin;
pub(crate) mod responses;

pub(crate) async fn https_accept_loop(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    state: Arc<AdminRuntimeState>,
) -> Result<()> {
    let app = build_app(state);
    loop {
        let (stream, _) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let app = app.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_connection(stream, acceptor, app).await {
                error!("backend HTTPS connection error: {e:#}");
            }
        });
    }
}

/// Compose the admin router (if enabled), the runtime router, and the
/// SPA fallback into one top-level `Router<()>` ready to serve.
fn build_app(state: Arc<AdminRuntimeState>) -> Router {
    let runtime: Router<Arc<AdminRuntimeState>> = runtime_router();

    if state.admin_enabled {
        let admin: Router<Arc<AdminRuntimeState>> = admin_router(state.clone());
        Router::new()
            .merge(runtime)
            .merge(admin)
            .fallback(spa_fallback_handler)
            .with_state(state)
    } else {
        Router::new()
            .merge(runtime)
            .fallback(admin_disabled_fallback)
            .with_state(state)
    }
}

/// Top-level fallback when the admin UI is enabled. Anything that
/// reaches this is either a SPA path (`/`, `/users`, `/assets/...`) or
/// an unknown JSON path; `serve_web_request` decides via its own
/// history-fallback rules.
async fn spa_fallback_handler(
    State(state): State<Arc<AdminRuntimeState>>,
    request: Request,
) -> AxumResponse {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    if crate::web::is_spa_request(&method, &path) {
        return crate::web::serve_web_request(&method, &path, &state.web_source).into_response();
    }
    error_response(StatusCode::NOT_FOUND, "not_found", "unknown endpoint").into_response()
}

async fn admin_disabled_fallback() -> AxumResponse {
    error_response(
        StatusCode::NOT_FOUND,
        "not_found",
        "admin interface is disabled",
    )
    .into_response()
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    acceptor: TlsAcceptor,
    app: Router,
) -> Result<()> {
    let tls_stream = acceptor
        .accept(stream)
        .await
        .context("accepting HTTPS connection")?;
    let peer_certs = PeerCertificates(Arc::new(
        tls_stream
            .get_ref()
            .1
            .peer_certificates()
            .map(|certs| certs.to_vec())
            .unwrap_or_default(),
    ));

    // Layer the per-connection extension onto a fresh clone of the
    // composed app. `Extension::layer(...)` is the standard Axum idiom
    // for "make this value available to every handler in the tree".
    let per_conn_app = app.layer(Extension(peer_certs));

    ServerBuilder::new(TokioExecutor::new())
        .serve_connection(
            TokioIo::new(tls_stream),
            TowerToHyperService::new(per_conn_app),
        )
        .await
        .map_err(|e| anyhow::anyhow!("serving HTTPS connection: {e}"))?;
    Ok(())
}
