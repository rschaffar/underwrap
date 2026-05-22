# Task: Drop unused `_now_epoch` parameter from the overview helper

Parent: `1300_story_backend_backendd_refinement.md`
Resolves: F-003 (review report `backend_backendd_2026-05-22.md`)

## Background

- `crates/backend-backendd/src/admin_api/overview.rs:21-25` —
  `admin_ui_overview_for_store(_, _, _now_epoch: u64)` ignores the
  parameter (the `_` prefix is the giveaway).
- `crates/backend-backendd/src/admin_api/overview.rs:64-68, 85` —
  the caller still computes `now_epoch` via `SystemTime::now()`
  just to pass it.

The "UnlockedUntil" bucket may have used the clock value earlier;
today the function only counts mode variants and per-permission
`writable` flags, neither of which needs the time.

## Scope

- Remove the `_now_epoch: u64` parameter from
  `admin_ui_overview_for_store`.
- Remove the `now_epoch` computation from
  `handle_admin_ui_overview` (lines 64-68).
- Drop the parameter from any test caller in
  `crates/backend-backendd/src/admin_api/`.

## Acceptance criteria

- [ ] `rg -n '_now_epoch' crates/backend-backendd/src/` returns
      nothing.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
