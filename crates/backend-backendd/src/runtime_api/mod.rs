//! Runtime mTLS endpoints — the surface that frontend daemons speak.
//!
//! Three endpoints, all addressed by `wrapper_name` + `resource_name`:
//!
//! - `POST /v1/wrappers` (see [`wrappers::handle_runtime_wrapper_register`]).
//! - `POST /v1/wrapper-invocations` (see
//!   [`invocations::handle_runtime_wrapper_invocation_open`]).
//! - `POST /v1/wrapper-invocations/finalize` (see
//!   [`invocations::handle_runtime_wrapper_invocation_finalize`]).
//!
//! [`handle_service_request`] is the URL dispatcher.

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use std::sync::Arc;
use tracing::warn;

use crate::http::responses::{error_response, parse_json_body, store_locked_response};
use crate::runtime_state::AdminRuntimeState;
use crate::store_registry::RegistryLookupError;

pub(crate) mod invocations;
pub(crate) mod principal;
pub(crate) mod wrappers;

use principal::resolve_runtime_client;

pub(crate) async fn handle_service_request(
    request: Request<Incoming>,
    state: Arc<AdminRuntimeState>,
    peer_certificates: Arc<Vec<rustls::pki_types::CertificateDer<'static>>>,
) -> std::result::Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let resolved = match resolve_runtime_client(&state, peer_certificates.as_ref()) {
        Ok(resolved) => resolved,
        Err(e) => {
            warn!(
                method = %request.method(),
                path = %request.uri().path(),
                err = format!("{e:#}"),
                "rejecting runtime client request: {e:#}"
            );
            return Ok(error_response(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "runtime client is not currently authorized",
            ));
        }
    };
    let principal = resolved.instance_id;
    let store = match state
        .stores
        .read()
        .unwrap()
        .unlocked_backend(&resolved.store_id)
    {
        Ok(b) => b,
        Err(RegistryLookupError::Locked) => {
            return Ok(store_locked_response(&resolved.store_id));
        }
        Err(RegistryLookupError::NotFound) => {
            return Ok(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "runtime client mapping points at an unknown store",
            ));
        }
    };
    let response = match (request.method(), request.uri().path()) {
        (&Method::POST, "/v1/wrappers") => match parse_json_body::<
            underwrap_protocol::service::RuntimeWrapperRegisterRequest,
        >(request)
        .await
        {
            Ok(body) => {
                wrappers::handle_runtime_wrapper_register(&state, store.as_ref(), &principal, &body)
            }
            Err(response) => response,
        },
        (&Method::POST, "/v1/wrapper-invocations") => match parse_json_body::<
            underwrap_protocol::service::RuntimeWrapperInvocationOpenRequest,
        >(request)
        .await
        {
            Ok(body) => invocations::handle_runtime_wrapper_invocation_open(
                store.as_ref(),
                &principal,
                &body,
            ),
            Err(response) => response,
        },
        (&Method::POST, "/v1/wrapper-invocations/finalize") => match parse_json_body::<
            underwrap_protocol::service::RuntimeWrapperInvocationFinalizeRequest,
        >(request)
        .await
        {
            Ok(body) => invocations::handle_runtime_wrapper_invocation_finalize(
                store.as_ref(),
                &principal,
                &body,
            ),
            Err(response) => response,
        },
        _ => error_response(StatusCode::NOT_FOUND, "not_found", "unknown endpoint"),
    };
    Ok(response)
}
