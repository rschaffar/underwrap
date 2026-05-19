# Task: Wrapper writeback test coverage

Parent: `410_story_wrapper_buffered_writeback_and_warnings.md`

## Scope

Integration tests against a real `underwrap-backendd` + `underwrapd`
for the full wrapper-side write-back loop. VM coverage lives in
task 432; this task is for cargo-level integration tests that don't
need cap_sys_admin.

### Test cases (in `crates/integration-tests/`)

New file `tests/wrapper_writeback.rs`:

- `clean_run_no_mutations_sends_only_usage_signal` — wrapper exits
  without changing anything; backend blob bytes unchanged; pair
  `last_used_at` updated (when task 421 lands; for now assert the
  audit trail).
- `mutations_with_writable_grant_persist_via_checkin` — secretsd
  CreateItem + tmpfs file write; on exit, backend blob contains
  the new state.
- `mutations_with_readonly_grant_drop_locally` — same mutations,
  grant has `writable=false`; backend blob unchanged; journal
  contains `writeback_rejected` event; stderr contains the
  human-readable line.
- `suppress_env_var_hides_stderr_line` — same as above but with
  `UNDERWRAP_SUPPRESS_READONLY_WARNING=1`; journal still contains
  the event; stderr does not.
- `server_rejects_checkin_after_grant_flip` — grant flipped from
  writable→read-only between Checkout and Checkin via an admin
  call; wrapper observes `Error::Unauthorized`, emits the
  `server_rejected` warning, exits cleanly.
- `target_inherits_no_underwrap_env_vars` — assert the target
  process does not see `UNDERWRAP_SUPPRESS_READONLY_WARNING` or
  any other `UNDERWRAP_*` var (`env_clear` whitelist regression).
- `non_canonical_checkin_rejected_by_server` — wrapper sends
  manually-tweaked non-canonical bytes (test-only path);
  server rejects; wrapper warns; backend unchanged.

### `frontend-secretsd` unit tests

Round-trip tests already in scope of task 411 — this task adds
the end-to-end glue.

### `frontend-ns` unit tests

Decision-tree tests already in scope of task 413.

## Acceptance criteria

- [ ] All tests above land and pass.
- [ ] Tests assert on structured journal events via a tracing
      subscriber, not via fragile stderr parsing.
- [ ] Stderr assertions match suppression behavior.
- [ ] `cargo test --workspace` green; clippy clean.

## Out of scope

- VM test — task 432.
- last_used assertions — task 423.
