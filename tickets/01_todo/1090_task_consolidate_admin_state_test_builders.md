# Task: Consolidate the `AdminRuntimeState` test-state builders

Parent: `1000_story_backend_backendd_refinement.md`
Resolves: F-011 (review report `backend-backendd_2026-05-21.md`)

## Background

`crates/backend-backendd/src/test_support.rs` is 753 lines, and the
biggest reason is the five near-duplicate `AdminRuntimeState`
builders:

- `test_admin_state` (lines ~163-192)
- `seeded_admin_state` (lines ~216-253)
- `service_state_with_runtime_client` (lines ~286-350)
- `fresh_bootstrap_state` (lines ~361-385)
- `crud_state` (lines ~492-516)

Each restates every field of `AdminRuntimeState`. Adding a field to
the struct means touching every builder. The per-builder variation
(seed a user, wire up a client instance, set
`stores_data_root`, …) is small.

## Scope

In `crates/backend-backendd/src/test_support.rs`:

- Add one `pub(crate) fn default_admin_runtime_state() -> AdminRuntimeState`
  that returns the canonical "no stores, no client instances,
  empty registry, admin enabled, embedded SPA, throw-away session
  key" defaults that every existing builder restates.
- Rewrite the five builders to call the default and then mutate
  the two or three fields they actually care about. For example,
  `seeded_admin_state` becomes ~10 lines: build default, populate
  the user file, hand-craft the session key, return.

Pick the shape that reads cleanest:

- a single `default_admin_runtime_state()` returning the struct,
  callers mutate with `let mut s = default_admin_runtime_state();
  s.users_file = …; s`, **or**
- a tiny builder struct with two or three setters for the fields
  the five callers actually need.

Either is fine; pick one and stay consistent.

Target: shrink `test_support.rs` to ~500 lines.

## Out of scope

- Changing the shape of `AdminRuntimeState` itself. The epic
  explicitly defers that.
- Touching the `RuntimeApiFixture`, `MemTestStore`, `TestStore`,
  or header / response helpers in the same file. They aren't
  duplicated the same way.
- Splitting `test_support.rs` into multiple files. If the
  consolidation lands the file under ~500 lines, leave it as one
  file.

## Acceptance criteria

- [ ] One `default_admin_runtime_state` (or builder) helper exists.
- [ ] The five named builders each call into it and only set the
      fields specific to their test scenario.
- [ ] `test_support.rs` is meaningfully shorter (target ≤ 600
      lines; ~500 is ideal).
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
