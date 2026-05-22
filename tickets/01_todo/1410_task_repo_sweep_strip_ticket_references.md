# Task: Repo sweep — strip every ticket / epic / story pointer from source, config, and docs

Parent: `1300_story_backend_backendd_refinement.md`
Resolves: F-019 (review report `backend_backendd_2026-05-22.md`)

## Background

`CLAUDE.md` "Posture" + the `underwrap-component-refine` skill rule:
"tickets and commit messages are the right place for that context;
the code is not." 32 occurrences survive across the repo outside
the `tickets/` tree itself.

## Scope

For every match, drop the ticket / epic / story pointer. Where the
parenthetical is the only content of the line or comment, drop the
whole line/comment. Where the comment is load-bearing apart from
the pointer (e.g. "the backend's CSRF double-submit check
(ticket 212) passes"), keep the descriptive text and drop only the
parenthetical.

### Hit list (from the 2026-05-22 review)

In-scope (this crate):
- `crates/backend-backendd/src/runtime_state.rs:13` —
  `epic_backend_modularization` (likely already dropped by 1390's
  F-016 sub-bullet; verify before this ticket runs).
- `crates/backend-backendd/tests/ts_export.rs:171, 191` — ticket
  384, ticket 404.

Out-of-scope but part of this sweep:
- `web/playwright.config.ts:38` — ticket 252.
- `web/e2e/secrets.spec.ts:2` — ticket 300.
- `web/e2e/profile.spec.ts:2` — ticket 294.
- `web/e2e/auth-flows.spec.ts:4` — ticket 224.
- `web/e2e/backendd.ts:73` — ticket 234.
- `web/e2e/dashboard.spec.ts:2` — ticket 294.
- `web/e2e/users.spec.ts:2` — ticket 293.
- `web/e2e/stores.spec.ts:2, 73, 249` — tickets 233, 234, 240.
- `web/e2e/multi-user.spec.ts:2` — ticket 243.
- `web/src/api/client.ts:293, 412, 501, 557` — tickets 231, 240,
  300, 384.
- `nix/tests/node.nix:271` — ticket 262.
- `nix/tests/test.py:111, 369, 1139, 1172, 1921` — tickets 212, 231,
  432, story 410, `epic_runtime_api_redesign`.
- `docs/backend.md:62` — ticket 230.
- `crates/shared-store/src/resource_blob/tests.rs:102, 127` — ticket 405.
- `crates/integration-tests/src/harness/readiness.rs:75, 90` —
  tickets 280, 281.
- `crates/integration-tests/src/tests/wrapper_writeback.rs:467` —
  ticket 423.

### Verification grep

Before closing the ticket, run:

```
rg -n -i '(ticket[[:space:]]*[0-9]{3,4}|epic_[a-z_]+|story_[a-z_]+|TODO\([0-9]{3,4}\)|FIXME\([0-9]{3,4}\))' \
  crates/ web/ nix/ docs/ examples/
```

Expect zero hits.

## Out of scope

- The `tickets/` tree itself (that's where these references belong).
- Commit messages and PR descriptions in git history.
- Stripping non-ticket references like "see RFC 8785" or
  "(see security.md)" — only ticket / epic / story pointers.

## Acceptance criteria

- [ ] The verification grep above returns nothing under `crates/`,
      `web/`, `nix/`, `docs/`, `examples/`.
- [ ] `cargo test --workspace` passes.
- [ ] `cargo fmt -- --check` passes.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` passes.
- [ ] `nix build .#checks.x86_64-linux.vm-test` passes (the
      `nix/tests/test.py` edits could subtly affect log assertions —
      VM test catches it).
- [ ] `cd web && npm test` passes (Vitest unit tests).
