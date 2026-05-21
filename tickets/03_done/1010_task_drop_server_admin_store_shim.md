# Task: Drop the `crate::server::admin_store` re-export shim

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-001 (review report `backend-backendd_2026-05-21.md`)

## Background

`server.rs:38-42` carries a stop-gap re-export with an explicit
"until then" comment:

```rust
// `admin_store` is re-exported because most `admin_api/*` modules
// still import it as `crate::server::admin_store`. The function lives
// next to its production code in `admin_api::stores`; this re-export
// is a stop-gap until those imports get updated.
pub(crate) use crate::admin_api::stores::admin_store;
```

There are 21 `use crate::server::admin_store;` statements across
`admin_api/{pairs,secrets,resources,policy}.rs`. The "until then"
never came; this task does the rewrite.

## Scope

For each file under `crates/backend-backendd/src/admin_api/`:

- Replace in-function `use crate::server::admin_store;` lines with
  one `use crate::admin_api::stores::admin_store;` at module top
  (next to the other module-level imports).
- File list with current counts:
  - `pairs.rs` (3 uses)
  - `secrets.rs` (8 uses)
  - `resources.rs` (6 uses)
  - `policy.rs` (5 uses)

Then in `crates/backend-backendd/src/server.rs`:

- Delete lines 38-42 (the comment block and the `pub(crate) use`).

## Out of scope

- Other items in the `pub use` block at the top of `server.rs`.
  Those are external-consumer re-exports; ticket 1110 handles them.
- Refactoring the handler bodies that imported `admin_store`. Pure
  import cleanup only.

## Acceptance criteria

- [ ] `grep -rn "crate::server::admin_store" crates/backend-backendd/src/`
      returns no matches (apart from the deleted lines).
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
