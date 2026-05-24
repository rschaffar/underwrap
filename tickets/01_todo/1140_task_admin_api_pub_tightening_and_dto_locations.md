# Task: Tighten admin_api `pub` surface; relocate inline DTOs

Resolves: B-ADM-3, B-ADM-11, B-ADM-12, B-ADM-27.

## Problem

`admin_api` is `pub(crate) mod admin_api` in `lib.rs:1`, but inside
the subtree several modules and DTOs are declared `pub`, suggesting
a wider audience than they have. Plus a handful of small naming/
visibility tics.

## What to do

1. **Demote module and DTO visibility** (B-ADM-3):
   - `crates/backend-backendd/src/admin_api/auth/mod.rs:20` —
     `pub mod bootstrap` → `pub(crate) mod bootstrap`.
   - In the same file at `:43,49,54,63`: `pub struct LoginRequest`,
     `LoginResponse`, `WhoamiResponse`, `EmptyObject` (deleted in
     1030 anyway) → `pub(crate)`.
   - `crates/backend-backendd/src/admin_api/auth/bootstrap.rs:29,35,40`:
     `BootstrapRequest`, `BootstrapStatusResponse`, `BootstrapResponse`
     → `pub(crate)`.
   - `crates/backend-backendd/src/admin_api/mod.rs:36` —
     `pub mod auth` → `pub(crate) mod auth`.
   - `crates/backend-backendd/src/admin_api/stores/lifecycle.rs:583`
     — `StoreLockResponse` → moved out by 1020 anyway; if 1020 has
     already landed, this is a no-op here.

   Note: `lib.rs:23-26` currently `pub use`s several of these. That
   block is deleted by `1240_task_lib_rs_pub_cleanup` — sequence so
   1140 lands after 1240, or do both at once. `pub use` of a
   `pub(crate)` item is legal as long as the re-export's containing
   item doesn't escape the crate.

2. **`AdminError::new` → private** (B-ADM-11):
   `crates/backend-backendd/src/admin_api/error.rs:37-47` —
   the function has no callers outside the `impl AdminError` block.
   Drop the `pub(crate)` qualifier so it's a private inherent method.

3. **Decide `store_not_found` symmetry** (B-ADM-12):
   `crates/backend-backendd/src/admin_api/error.rs:57-59` defines
   `AdminError::store_not_found()` while
   `not_found("unknown client instance")` /
   `not_found("unknown resource")` are open-coded elsewhere. Pick one:
   - Add peer helpers `client_instance_not_found`, `resource_not_found`,
     `pair_not_found` for naming symmetry.
   - Or inline `store_not_found` at its 6 call sites
     (`stores/crud.rs:251,359`, `stores/lifecycle.rs:73,413,552`,
     `stores/mod.rs:44`) and let `not_found("unknown store")` win.
   Recommendation: add peer helpers — the typed factory reads better
   in handlers.

4. **Hoist inline `use` statements to file-top** (B-ADM-27):
   `use underwrap_protocol::…` statements stashed inside handler
   bodies at:
   - `admin_api/pairs.rs:80,156`
   - `admin_api/resources.rs:90,153,263`
   - `admin_api/auth/mod.rs:208-209`
   Move them to each file's top-level `use` block.

## Acceptance

- `cargo build -p backend-backendd`
- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
