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
//! [`runtime_router`] is the entry point: it wires every endpoint into
//! an `axum::Router`, with a per-request middleware that resolves the
//! mTLS principal from the connection-attached `PeerCertificates`
//! extension and looks up the live backend.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::body::to_bytes;
use axum::extract::{FromRequest, Request, State};
use axum::routing::post;
use tracing::warn;

use crate::http::responses::Body;

/// Base64 plus JSON envelope bound for runtime writeback blobs. Small runtime
/// control requests continue using the shared 2 MiB extractor bound.
const MAX_RESOURCE_REQUEST_BODY_BYTES: usize =
    underwrap_protocol::resource_blob::MAX_CANONICAL_RESOURCE_BYTES.div_ceil(3) * 4 + 64 * 1024;

struct ResourceBody<T>(T);

impl<S, T> FromRequest<S> for ResourceBody<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = RuntimeError;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let bytes = to_bytes(req.into_body(), MAX_RESOURCE_REQUEST_BODY_BYTES)
            .await
            .map_err(|_| {
                RuntimeError::bad_request(
                    "invalid_request",
                    "resource request body exceeds its limit",
                )
            })?;
        serde_json::from_slice(&bytes)
            .map(Self)
            .map_err(|_| RuntimeError::bad_request("invalid_request", "invalid JSON request body"))
    }
}
use crate::runtime_api::error::RuntimeError;
use crate::runtime_state::ListenerState;
use crate::store_registry::RegistryLookupError;

pub(crate) mod error;
pub(crate) mod invocations;
pub(crate) mod principal;
pub(crate) mod wrappers;

use principal::resolve_runtime_client;

/// Connection-level extension carrying the rustls peer certificate
/// chain. Inserted once per connection by the HTTPS accept loop.
#[derive(Clone)]
pub(crate) struct PeerCertificates(pub(crate) Arc<Vec<rustls::pki_types::CertificateDer<'static>>>);

pub(crate) fn runtime_router() -> Router<Arc<ListenerState>> {
    Router::new()
        .route("/v1/wrappers", post(axum_wrappers_register))
        .route("/v1/wrapper-invocations", post(axum_invocation_open))
        .route(
            "/v1/wrapper-invocations/finalize",
            post(axum_invocation_finalize),
        )
}

/// Resolve the live runtime client + store from the request's
/// `PeerCertificates` extension. Returns either a ready-to-use principal
/// and unlocked store, or the canonical error response.
fn resolve_runtime_request(
    state: &ListenerState,
    peer_certs: &PeerCertificates,
) -> Result<(String, crate::store_registry::StoreLease), RuntimeError> {
    let resolved = resolve_runtime_client(state, peer_certs.0.as_ref()).map_err(|e| {
        warn!(err = format!("{e:#}"), "rejecting runtime client request");
        RuntimeError::unauthorized("unauthorized", "runtime client is not currently authorized")
    })?;
    let store = match state
        .shared
        .stores
        .read()
        .unwrap()
        .lease(&resolved.store_id)
    {
        Ok(b) => b,
        Err(RegistryLookupError::Locked) => {
            return Err(RuntimeError::store_locked(&resolved.store_id));
        }
        Err(RegistryLookupError::Busy) => return Err(RuntimeError::store_busy()),
        Err(RegistryLookupError::RecoveryRequired) => {
            return Err(RuntimeError::migration_recovery_required());
        }
        Err(RegistryLookupError::NotFound) => {
            return Err(RuntimeError::internal(
                "store lookup",
                "runtime client mapping points at an unknown store",
            ));
        }
    };
    Ok((resolved.instance_id, store))
}

async fn axum_wrappers_register(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(peer_certs): axum::Extension<PeerCertificates>,
    Body(parsed): Body<underwrap_protocol::service::RuntimeWrapperRegisterRequest>,
) -> Result<Json<underwrap_protocol::service::RuntimeWrapperRegisterResponse>, RuntimeError> {
    let (principal, store) = resolve_runtime_request(&state, &peer_certs)?;
    wrappers::handle_runtime_wrapper_register(&state, store.as_ref(), &principal, &parsed)
}

async fn axum_invocation_open(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(peer_certs): axum::Extension<PeerCertificates>,
    Body(parsed): Body<underwrap_protocol::service::RuntimeWrapperInvocationOpenRequest>,
) -> Result<Json<underwrap_protocol::service::RuntimeWrapperInvocationOpenResponse>, RuntimeError> {
    let (principal, store) = resolve_runtime_request(&state, &peer_certs)?;
    invocations::handle_runtime_wrapper_invocation_open(store.as_ref(), &principal, &parsed)
}

async fn axum_invocation_finalize(
    State(state): State<Arc<ListenerState>>,
    axum::Extension(peer_certs): axum::Extension<PeerCertificates>,
    ResourceBody(parsed): ResourceBody<
        underwrap_protocol::service::RuntimeWrapperInvocationFinalizeRequest,
    >,
) -> Result<Json<underwrap_protocol::service::RuntimeWrapperInvocationFinalizeResponse>, RuntimeError>
{
    let (principal, store) = resolve_runtime_request(&state, &peer_certs)?;
    invocations::handle_runtime_wrapper_invocation_finalize(store.as_ref(), &principal, &parsed)
}
