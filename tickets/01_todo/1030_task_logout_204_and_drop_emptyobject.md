# Task: Return 204 from logout; delete `EmptyObject`

Resolves: S-3.

## Problem

`handle_admin_logout` in `crates/backend-backendd/src/admin_api/auth/mod.rs:152-164`
returns `Json<EmptyObject>` — which produces HTTP 200 with body `{}`.
The `#[utoipa::path]` annotation at `:63` declares the response as
`status = 204`. The spec and the wire disagree.

The SPA's `postLogout` survives only because `unwrap`'s `(undefined as
unknown) as T` fallback masks the discrepancy.

## What to do

1. Change `handle_admin_logout` to return
   `axum::http::StatusCode::NO_CONTENT` directly (matching the
   declared 204).
2. Delete `pub struct EmptyObject` in `admin_api/auth/mod.rs:63`.
3. Update the `#[utoipa::path]` response if needed so the regenerated
   OpenAPI matches.
4. Regenerate `web/src/api/schema.ts`; the SPA already handles 204
   correctly.

## Acceptance

- `cargo test -p backend-backendd`
- `cargo test -p integration-tests`
- `cd web && npm test`
- After regen, `schema.ts` shows logout's response as 204 / no content.
