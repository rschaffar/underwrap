# Task: Split `service.rs` into admin and runtime protocol modules

## Problem

`crates/shared-protocol/src/service.rs` (~740 lines) holds two
unrelated protocols in one file:

- **Admin API** (~lines 1–622): the SPA ↔ backend surface. Session-cookie
  auth, CSRF, `utoipa::ToSchema` for OpenAPI/TS codegen, large and
  evolving (`Admin*` DTOs for users, stores, secrets, libsecret,
  wrapper permissions, resources, pairs, aggregate views).
- **Runtime API** (~lines 624–740): the frontend daemon ↔ backend
  surface. mTLS auth, exactly three endpoints
  (`Runtime{WrapperRegister,WrapperInvocationOpen,
  WrapperInvocationFinalize}{Request,Response}` plus
  `RuntimeWrapperRegisterPair`).

These have different clients, different auth, different trust
boundaries, and different stability concerns. The runtime types
deliberately do **not** derive `utoipa::ToSchema` (they are not in the
admin OpenAPI surface); the admin types all do. Co-locating them
obscures that boundary.

## What to do

1. Split `service.rs` into two submodules under a `service/` directory,
   keeping the public path `underwrap_protocol::service::*` stable so no
   downstream import churns:
   - `service/admin.rs` — every `Admin*` DTO and the
     `ServiceWrapperResourcePair` / aggregate-view types the admin
     surface uses.
   - `service/runtime.rs` — the `Runtime*` request/response types and
     `RuntimeWrapperRegisterPair`.
   - `service.rs` (or `service/mod.rs`) re-exports both with
     `pub use admin::*; pub use runtime::*;` so existing
     `use underwrap_protocol::service::Foo` keeps compiling.
   - Keep `ServiceErrorResponse` where both can see it (in
     `mod.rs`/`service.rs` root, or wherever is least surprising).
2. Move the `#[cfg(test)] mod tests;` to follow the
   `<stem>/tests.rs` convention for whichever file keeps the existing
   `service/tests.rs`; if both modules need tests, give each its own
   sibling `tests.rs`.
3. This is a pure code-organization change — **no wire change, no type
   rename**. Verify the OpenAPI export is byte-identical before/after.

## Acceptance

- `cargo test --workspace`
- `cargo test --test openapi_export` (or whatever produces the SPA
  schema JSON) — diff must be empty.
- `cargo clippy --all-targets --workspace -- -D warnings`
- `cargo fmt -- --check`
- `cd web && npm run build` (codegen unaffected).
