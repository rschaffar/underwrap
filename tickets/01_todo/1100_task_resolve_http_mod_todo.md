# Task: Resolve the `http/mod.rs` TODO about transport wiring

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-012 (review report `backend-backendd_2026-05-21.md`)

## Background

`crates/backend-backendd/src/http/mod.rs:11-13` says:

```text
//! The transport-level wiring (TLS accept loop, admin-vs-service
//! request split) currently still lives in `server.rs`; it will move
//! here in a later modularization step.
```

The original modularization epic (`epic_backend_modularization.md`)
targeted this move — its `http/mod.rs` was supposed to own
`https_accept_loop`, `handle_https_connection`, and
`handle_https_request`. The move never happened; those three live
in `server.rs:142-207`.

This needs a user decision before the task starts.

## Two options

**Option 1 — move the wiring (per the original epic).**

- Move `https_accept_loop`, `handle_https_connection`, and
  `handle_https_request` from `server.rs:142-207` into
  `http/mod.rs` (or a new `http/listener.rs`).
- Leave `pub async fn run(...)` in `server.rs`; it calls into
  `http::run()` (or whatever the moved entry point is called).
- Update `server.rs`'s `use crate::http::…` imports.
- Update the TODO in `http/mod.rs` to describe the module's actual
  contents.
- **Run the VM test** in addition to the default gates — this is
  the transport boundary.

**Option 2 — drop the TODO, keep the wiring in `server.rs`.**

- Delete the trailing sentence in `http/mod.rs:11-13`.
- Update the module rustdoc to honestly describe what `http/`
  contains now (responses, origin — not the transport wiring).
- Default gates only.

## Scope

Before starting, **ask the user which option to take**. The
modularization epic's design intent favours Option 1; but
`server.rs` is currently only 207 lines, so option 2 may also be
defensible. Don't pick a side unilaterally.

Once the choice is made, do exactly the work above for that
option. Don't bundle additional refactoring.

## Out of scope

- Splitting `http/mod.rs` into more granular sub-modules.
- Changing the actual TLS / hyper plumbing semantics.

## Acceptance criteria

- [ ] The discrepancy between `http/mod.rs`'s docstring and the
      tree's actual shape is gone (either by moving code or by
      rewriting the docstring).
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
- [ ] If Option 1 was taken,
      `nix build .#checks.x86_64-linux.vm-test` passes.
