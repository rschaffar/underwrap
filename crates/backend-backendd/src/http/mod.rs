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
//! The transport-level wiring (TLS accept loop, admin-vs-service
//! request split) currently still lives in `server.rs`; it will move
//! here in a later modularization step.

pub(crate) mod origin;
pub(crate) mod responses;
