# Task: Repo sweep — relocate inline tests where tests ≥ production

Parent: `1130_story_admin_api_refinement.md`
Resolves: F-011 (review report `admin_api_2026-05-22.md`)

## Background

`CLAUDE.md` mandates the sibling-file `tests.rs` submodule pattern,
with an exception for "one or two tiny tests where a separate file
would be ceremony for five lines." The rule: tests longer than the
production code → split.

The previous epic's ticket 1080 split the backend-backendd-side
inline blocks. The same lens applied repo-wide flags three more
modules where tests substantially exceed production:

- `crates/shared-store/src/plain.rs` — prod=57, tests=165 (3×
  production).
- `crates/shared-store/src/resource_blob.rs` — prod=278, tests=315
  (tests > production; whole file 593 lines).
- `crates/frontend-underwrapd/src/wrapper_bindings.rs` — prod=25,
  tests=50 (2× production).

Two borderlines (`shared-store/src/path.rs` prod=159 tests=128,
`frontend-ns/src/env.rs` prod=69 tests=63) are left alone — both sit
under parity and reasonable people can disagree.

## Scope

For each of the three files above:

- Create a sibling `<name>/tests.rs` next to the production file (or
  a sibling `tests.rs` in the same module directory if the production
  file is the directory's `mod.rs` — neither of the three is). For
  `plain.rs`, create `crates/shared-store/src/plain/tests.rs`. Same
  shape for the other two.
- Move the entire body of the inline `#[cfg(test)] mod tests { ... }`
  block (everything between `mod tests {` and the matching `}`) into
  that new file, **without** the `mod tests {` wrapper itself.
- Replace the inline block in the production file with a one-liner:

  ```rust
  #[cfg(test)] mod tests;
  ```

- If the moved tests use `use super::*;`, that still works — the new
  `tests.rs` is a submodule of the production module.
- If the moved tests reach for items via `crate::...`, those imports
  still resolve.

Do **not** touch the borderline files. Do **not** sweep for other
inline blocks under the threshold.

## Out of scope

- Refactoring the moved tests.
- Splitting `tests.rs` files further (none of these new files will
  hit the 600-line ceiling: 165, 315, 50).
- Adding a `test_support.rs` — no helper duplication justifies one
  for just these three files.

## Acceptance criteria

- [ ] `crates/shared-store/src/plain.rs` ends with
      `#[cfg(test)] mod tests;` and no inline block.
- [ ] `crates/shared-store/src/plain/tests.rs` exists with the moved
      tests.
- [ ] Same for `resource_blob.rs` / `resource_blob/tests.rs`.
- [ ] Same for `wrapper_bindings.rs` / `wrapper_bindings/tests.rs`.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes (specifically, the moved tests
      still run — verify with `cargo test -p shared-store` and
      `cargo test -p frontend-underwrapd`).
