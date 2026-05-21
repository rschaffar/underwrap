//! HTTP-layer plumbing for the backend listener.
//!
//! Submodules:
//!
//! - [`responses`] — small helpers that every handler uses to emit
//!   `Response<Full<Bytes>>` values (`json_response`,
//!   `error_response`, `store_locked_response`, `parse_json_body`).
//! - [`origin`] — `Origin` / `Referer` allowlisting on state-changing
//!   requests + the matching configuration-validation helper.
//!
//! This module also owns the TLS accept loop and the
//! admin-vs-service request split:
//! [`https_accept_loop`] is what `server::run` spawns once it has
//! bound the listener and built the admin runtime state.

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::error;

use crate::http::responses::error_response;
use crate::runtime_api::handle_service_request;
use crate::runtime_state::AdminRuntimeState;

pub(crate) mod origin;
pub(crate) mod responses;

pub(crate) async fn https_accept_loop(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    state: Arc<AdminRuntimeState>,
) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let acceptor = acceptor.clone();
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_https_connection(stream, acceptor, state).await {
                error!("backend HTTPS connection error: {e:#}");
            }
        });
    }
}

async fn handle_https_connection(
    stream: tokio::net::TcpStream,
    acceptor: TlsAcceptor,
    state: Arc<AdminRuntimeState>,
) -> Result<()> {
    let tls_stream = acceptor
        .accept(stream)
        .await
        .context("accepting HTTPS connection")?;
    let peer_certificates = Arc::new(
        tls_stream
            .get_ref()
            .1
            .peer_certificates()
            .map(|certs| certs.to_vec())
            .unwrap_or_default(),
    );

    let service = service_fn(move |req| {
        let state = Arc::clone(&state);
        let peer_certificates = Arc::clone(&peer_certificates);
        async move { handle_https_request(req, state, peer_certificates).await }
    });

    hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(tls_stream), service)
        .await
        .context("serving HTTPS connection")?;
    Ok(())
}

async fn handle_https_request(
    request: Request<Incoming>,
    state: Arc<AdminRuntimeState>,
    peer_certificates: Arc<Vec<rustls::pki_types::CertificateDer<'static>>>,
) -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let path = request.uri().path();
    if path.starts_with("/v1/admin/") || crate::web::is_spa_request(request.method(), path) {
        if !state.admin_enabled {
            return Ok(error_response(
                StatusCode::NOT_FOUND,
                "not_found",
                "admin interface is disabled",
            ));
        }
        return crate::admin_api::handle_admin_request(request, state).await;
    }
    handle_service_request(request, state, peer_certificates).await
}
