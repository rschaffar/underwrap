# Task: Move `auth.rs` inline tests to `auth/tests.rs`

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-007 (review report `backend-backendd_2026-05-21.md`)

## Background

`crates/backend-backendd/src/auth.rs` is 700 lines, with an inline
`#[cfg(test)] mod tests { … }` block from line 407 to 700 (~293
lines of tests). `CLAUDE.md` mandates the sibling-file pattern:

> Use the sibling-file submodule pattern rather than inline
> `mod tests {}` blocks at the bottom of production files.
>
> Exceptions where inline `mod tests { ... }` is fine: modules with
> one or two tiny tests where a separate file would be ceremony for
> five lines.

The auth tests are nowhere near "two tiny tests". Every other
module in the crate that meets this bar (`csrf`, `bootstrap`,
`stores`, `client_instances`, `users` admin, `pairs`, `secrets`,
`resources`, `invocations`, `wrappers`, `principal`,
`store_registry`, `documents`, `http/origin`) already uses the
sibling pattern.

## Scope

In `crates/backend-backendd/src/auth.rs`:

- Replace the inline `mod tests { ... }` block (lines 407-700)
  with `#[cfg(test)] mod tests;`.

Create `crates/backend-backendd/src/auth/tests.rs`:

- Move the test body verbatim. The opening `use super::*;` reaches
  every private and `pub(crate)` item exactly like the inline
  block did. No additional re-exports needed.

No test removed, no test added, no rename. Pure move.

## Out of scope

- Re-shaping the test set, splitting into per-feature files, or
  introducing `test_support` helpers for auth-specific fixtures.
  If a follow-up sees value in any of that, file it separately.

## Acceptance criteria

- [ ] `crates/backend-backendd/src/auth.rs` ends with
      `#[cfg(test)] mod tests;` and no inline tests.
- [ ] `crates/backend-backendd/src/auth/tests.rs` exists and
      contains every test that was in the inline block.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test -p backend-backendd auth::` shows the same
      number of tests as before the split.
- [ ] `cargo test --workspace` passes.
