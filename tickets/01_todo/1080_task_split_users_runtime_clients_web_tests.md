# Task: Split inline tests in `users.rs`, `runtime_clients.rs`, `web.rs`

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-008 (review report `backend-backendd_2026-05-21.md`)

## Background

Three production modules in `backend-backendd` still carry inline
`mod tests` blocks despite having more test code than production
code (or close to it). The rest of the crate uses the sibling
pattern uniformly.

Counts:

- `crates/backend-backendd/src/users.rs` — 605 lines, ~209 lines of
  inline tests.
- `crates/backend-backendd/src/runtime_clients.rs` — 383 lines, ~130
  lines of inline tests.
- `crates/backend-backendd/src/web.rs` — 466 lines, ~250 lines of
  inline tests (more tests than production).

## Scope

For each file, perform the same mechanical move that
`1070_task_split_auth_tests_sibling.md` describes:

- Replace the inline `mod tests { ... }` block with
  `#[cfg(test)] mod tests;`.
- Create `<module>/tests.rs` next to it, containing the moved body.

File pairings:

- `src/users.rs` → `src/users/tests.rs`
- `src/runtime_clients.rs` → `src/runtime_clients/tests.rs`
- `src/web.rs` → `src/web/tests.rs`

If 1015 (drop v1 users migration) lands first, the v1 test fixture
is already gone; this task just moves what's left.

## Out of scope

- Changing test content. Pure move.
- Splitting any of the three production modules. The bodies stay
  where they are.

## Acceptance criteria

- [ ] None of the three production files contain
      `#[cfg(test)] mod tests { … }` blocks anymore — each ends
      with `#[cfg(test)] mod tests;`.
- [ ] Three new sibling `tests.rs` files exist.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes, with the same test count
      per module as before the move.
