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
use axum::extract::State;
use axum::routing::post;
use tracing::warn;

use crate::http::responses::Body;
use crate::runtime_api::error::RuntimeError;
use crate::runtime_state::AdminRuntimeState;
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

pub(crate) fn runtime_router() -> Router<Arc<AdminRuntimeState>> {
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
    state: &AdminRuntimeState,
    peer_certs: &PeerCertificates,
) -> Result<(String, Arc<dyn underwrap_store::StorageBackend>), RuntimeError> {
    let resolved = resolve_runtime_client(state, peer_certs.0.as_ref()).map_err(|e| {
        warn!(err = format!("{e:#}"), "rejecting runtime client request");
        RuntimeError::unauthorized("unauthorized", "runtime client is not currently authorized")
    })?;
    let store = match state
        .stores
        .read()
        .unwrap()
        .unlocked_backend(&resolved.store_id)
    {
        Ok(b) => b,
        Err(RegistryLookupError::Locked) => {
            return Err(RuntimeError::store_locked(&resolved.store_id));
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
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(peer_certs): axum::Extension<PeerCertificates>,
    Body(parsed): Body<underwrap_protocol::service::RuntimeWrapperRegisterRequest>,
) -> Result<Json<underwrap_protocol::service::RuntimeWrapperRegisterResponse>, RuntimeError> {
    let (principal, store) = resolve_runtime_request(&state, &peer_certs)?;
    wrappers::handle_runtime_wrapper_register(&state, store.as_ref(), &principal, &parsed)
}

async fn axum_invocation_open(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(peer_certs): axum::Extension<PeerCertificates>,
    Body(parsed): Body<underwrap_protocol::service::RuntimeWrapperInvocationOpenRequest>,
) -> Result<Json<underwrap_protocol::service::RuntimeWrapperInvocationOpenResponse>, RuntimeError> {
    let (principal, store) = resolve_runtime_request(&state, &peer_certs)?;
    invocations::handle_runtime_wrapper_invocation_open(store.as_ref(), &principal, &parsed)
}

async fn axum_invocation_finalize(
    State(state): State<Arc<AdminRuntimeState>>,
    axum::Extension(peer_certs): axum::Extension<PeerCertificates>,
    Body(parsed): Body<underwrap_protocol::service::RuntimeWrapperInvocationFinalizeRequest>,
) -> Result<Json<underwrap_protocol::service::RuntimeWrapperInvocationFinalizeResponse>, RuntimeError>
{
    let (principal, store) = resolve_runtime_request(&state, &peer_certs)?;
    invocations::handle_runtime_wrapper_invocation_finalize(store.as_ref(), &principal, &parsed)
}
