# Task: Test coverage for the new grant model and global write lock

Parent: `245_story_agent_grant_model_rewrite.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Rust integration tests under `crates/integration-tests/`:
  - Each of the 4 grant modes exercised end-to-end via the wrapper chain:
    - `Locked` → wrapper invocation denied
    - `Unlocked` → wrapper invocation succeeds
    - `UnlockedUntil { until }` → succeeds before deadline, denied after (use a short test deadline, no flaky sleeps)
    - `PerCallApproval { approval_key }` → succeeds with a valid ssh-agent signature; denied without
  - Ungranted `(wrapper, host, user)` tuple → 404 from the daemon
  - Duplicate grant tuple rejected on persist with the stable error code
  - Global `writes_enabled = false` blocks all writes:
    - admin UI write → 403 `writes_globally_disabled`
    - wrapper-agent write → daemon error path
  - Toggling `writes_enabled` re-enables writes immediately (no restart)
  - Old per-secret mutation-window endpoints return 404 (regression guard)
- VM test extension (`nix/tests/test.py`): exercise at least one wrapper grant with each mode that doesn't require a deadline, plus a global write-lock toggle round-trip on a real NixOS host.
- Vitest + Playwright tests for the SPA equivalent flows are delivered in ticket 250 (matrix tab) and ticket 252 (e2e); this ticket owns only the Rust + VM layers.

## Acceptance criteria

- [ ] All listed Rust tests pass under `cargo test --workspace`.
- [ ] VM test extended and green.
- [ ] No flake-prone sleeps; `UnlockedUntil` tests use a controllable clock or a short bounded wait verified with `expect` polling.
- [ ] Negative test: assert no `MutationState` / `SecretMutationWindow` types exist in the workspace (e.g. via a grep-style cargo test that fails if those identifiers reappear).
