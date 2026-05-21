# Task: Decide and lock the crate's public-API surface

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-002 (review report `backend-backendd_2026-05-21.md`)

## Background

`crates/backend-backendd/src/server.rs:22-36` has a `pub use` block
with the comment "Once the crate root grows its own re-exports we
can drop these — for now `server` remains the import path of
record." The block is not dead code — it's load-bearing. Three
external callers reach `BackendConfig`, `HttpsConfig`,
`HttpsTlsConfig`, `AdminConfig`, `BootstrapRequest`,
`BootstrapResponse`, `BootstrapStatusResponse`, `LoginRequest`,
`LoginResponse`, `WhoamiResponse`, `LoadedStoreRecord`,
`RegistryLookupError`, `StoreRegistry`, `StoreState`, `DEFAULT_STORES_FILE`,
`is_valid_store_id`, and `load_store_records` through
`underwrap_backendd::server::*`:

- `crates/integration-tests/src/harness/mod.rs`
- `crates/integration-tests/src/tests/daemon.rs`
- `crates/backend-backendd/tests/ts_export.rs`

Plus `crates/backend-backendd/src/main.rs:6` inside the same crate.

The block isn't wrong — it just doesn't match its own comment.

## Two options

**Option 1 — lift the re-exports to `lib.rs`.**

- Move the `pub use` block from `server.rs:22-36` to `lib.rs`.
- Update the three external callers and `main.rs` to import from
  `underwrap_backendd::*` directly (e.g. `use underwrap_backendd::BackendConfig`).
- Drop the "until then" comment.
- Optionally make the `config` / `store_registry` / `admin_api::auth`
  / `admin_api::bootstrap` modules `pub` at the crate root if a
  cleaner direct-import path makes sense; the `pub use` shape works
  too.

**Option 2 — keep `server.rs` as the path of record.**

- Just delete the "Once the crate root grows…" comment so the
  re-export block reads as an intentional public-API choice instead
  of a transitional stop-gap.

## Scope

Before starting, **ask the user which option to take**. Option 1 is
the more "Rust-idiomatic" path (the crate root usually owns the API
surface); Option 2 is the smaller diff and leaves `server::` as the
import path everywhere it already is.

Whatever option is picked, drop the misleading comment.

## Out of scope

- Re-shaping the API itself (which types are public, which fields
  are public). This is about the import path only.

## Acceptance criteria

- [ ] The "Once the crate root grows…" comment is gone.
- [ ] If Option 1: the four call-site files
      (`harness/mod.rs`, `tests/daemon.rs`, `ts_export.rs`,
      `main.rs`) import the lifted types from `underwrap_backendd`
      directly.
- [ ] If Option 2: the `pub use` block stays in `server.rs` with a
      rustdoc comment that honestly describes it as the public-API
      surface.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
