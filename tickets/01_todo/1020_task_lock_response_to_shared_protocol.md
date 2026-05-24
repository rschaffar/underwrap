# Task: Move `StoreLockResponse` to `shared-protocol::service`

Resolves: S-2.

## Problem

`StoreLockResponse { ok, was_locked }` is the only `Admin*Response` DTO
defined inline next to its handler. Every other Admin response type
lives in `underwrap_protocol::service`. The generated `schema.ts` then
exports one type with a different naming pattern than its peers.

Location: `crates/backend-backendd/src/admin_api/stores/lifecycle.rs:583-586`.

## What to do

1. Define `AdminStoreLockResponse` in `shared-protocol/src/service.rs`
   (mirror the shape of the existing `Admin*Response` types).
2. Update `crates/backend-backendd/src/admin_api/stores/lifecycle.rs`
   to import and return the new type; delete the inline `StoreLockResponse`.
3. Regenerate `web/src/api/schema.ts` (run whatever the project uses —
   typically `cargo test --test openapi_export` produces the JSON the
   SPA's codegen consumes, then `npm run codegen` or equivalent in
   `web/`).
4. Update SPA call sites if any imported the old name.

## Acceptance

- `cargo test --workspace`
- `cargo test --test openapi_export` (drift test green)
- `cd web && npm test && npm run build`
