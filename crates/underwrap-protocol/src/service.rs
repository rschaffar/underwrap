//! Service wire types, split by trust boundary into two submodules:
//!
//! - `admin` — the admin SPA ↔ backend surface (session-cookie auth,
//!   OpenAPI/`ToSchema`-bearing `Admin*` DTOs).
//! - `runtime` — the frontend daemon ↔ backend surface (mTLS, the
//!   `Runtime*` request/response types and reason vocabulary).
//!
//! Both are re-exported here so the canonical path stays
//! `underwrap_protocol::service::*` regardless of which submodule a type
//! lives in. [`ServiceErrorResponse`] is shared by both surfaces (the
//! admin error body and the daemon's runtime error classifier) and lives
//! at the module root.

use serde::{Deserialize, Serialize};

mod admin;
mod runtime;

pub use admin::*;
pub use runtime::*;

/// Generic structured error body returned by both the admin and runtime
/// surfaces. `code` is a stable machine token; `message` is human-facing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct ServiceErrorResponse {
    pub code: String,
    pub message: String,
}

#[cfg(test)]
mod tests;
