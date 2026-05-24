# Task: Collapse `admin_router_and_openapi` + drop BC wrapper

Resolves: B-ADM-1, B-ADM-2; one entry from B-DOC-2.

## Problem

`crates/backend-backendd/src/admin_api/mod.rs:87-140,166-182` has three
overlapping router-construction functions:

- `admin_router_and_openapi(state) -> (Router, OpenApi)` — only ever
  called by `admin_router` itself (`admin_router_and_openapi(state).0`).
  The OpenApi half is never consumed.
- `admin_router(state)` — a one-line forwarder whose docstring calls
  it a "Backwards-compatible wrapper that drops the OpenAPI document".
  Misleading in a greenfield codebase.
- `admin_openapi_spec()` — separately constructs the same router tree
  for the drift test. Two parallel constructions must stay in lockstep
  with no automated check.

Plus, `crates/backend-backendd/src/openapi.rs:19-21` defines
`openapi::admin_openapi_spec` as a one-line forwarder to
`admin_api::admin_openapi_spec`. Only consumer is `tests/openapi_export.rs`.

## What to do

1. Extract a `fn build_admin_openapi_router(state) ->
   OpenApiRouter<Arc<AdminRuntimeState>>` that does the merges of
   `build_public_router()` + `build_protected_router()` onto the
   `OpenApiRouter::with_openapi(ApiDoc::openapi())` base. Single
   source of truth.
2. `admin_router(state) -> Router` calls `build_admin_openapi_router`,
   applies the tower layers, calls `.split_for_parts().0`. Drop the
   tuple-returning wrapper entirely.
3. `admin_openapi_spec() -> OpenApi` calls `build_admin_openapi_router`
   (no layers needed for the spec) and returns `.split_for_parts().1`
   (or `.into_openapi()`).
4. Decide on the forwarder in `openapi.rs`:
   - Keep `pub use crate::admin_api::admin_openapi_spec` from
     `lib.rs` (since `lib.rs` is the public entry point).
   - Or delete `openapi.rs` if it no longer carries anything else.
5. Update the docstrings to describe the single construction.

## Acceptance

- `cargo test -p backend-backendd`
- `cargo test --test openapi_export`
- `cargo clippy --all-targets --workspace -- -D warnings`
