# Task: Remove dead `PolicyDocument::evaluate_permission` (with a security check)

## Problem

`PolicyDocument::evaluate_permission`
(`crates/shared-protocol/src/policy.rs:133`) has **zero callers** in
production or tests. Its `_at` sibling, `evaluate_permission_at`
(`policy.rs:121`), is used only by `evaluate_permission` (dead) and by
`policy/tests.rs`. The actual authorization path does **not** go through
either: `resolve_invocation`
(`crates/backend-backendd/src/runtime_api/invocations.rs:40`) calls
`policy.find_permission(...)` then `decide(permission, variant, now)`
directly.

## Security gate (do this FIRST, before deleting anything)

Before removing the helper, confirm we are not dropping a check that the
live path lacks. Verify all of the following:

1. The sole live authorization path for runtime invocations is
   `resolve_invocation` → `find_permission` → `decide`, and it is
   reached by **both** open and finalize handlers
   (`handle_runtime_wrapper_invocation_open` /
   `_finalize`). Confirm there is no other entry point that was relying
   on `evaluate_permission` for an authorization decision.
2. `evaluate_permission`/`evaluate_permission_at` perform **no**
   additional check beyond `find_permission` + `decide` — i.e. deleting
   them removes no logic, only an unused wrapper. (Read the bodies and
   confirm they are pure delegators.)
3. `writable_for` (`policy.rs:152`, used by integration tests) stays —
   it is referenced by `wrapper_permissions.rs` tests. Do not remove it.
4. Grep the whole workspace (including `web/`, nix tests, and
   integration-tests) once more to be certain nothing references
   `evaluate_permission` by name.

Record the outcome of these checks in the PR/commit message.

## What to do

1. Delete `PolicyDocument::evaluate_permission`.
2. Decide `evaluate_permission_at`'s fate:
   - If tests are its only remaining caller, either (a) keep it as a
     thin tested helper and add a one-line doc noting it mirrors the
     live `find_permission`+`decide` path, or (b) inline
     `decide(doc.find_permission(...).unwrap(), ...)` at the test call
     sites and delete it too. Prefer (b) if it does not make the tests
     noticeably noisier, so the test exercises the exact production
     composition.
3. Keep `decide`, `find_permission`, and `writable_for`.

## Acceptance

- The four security-gate checks above are confirmed and noted.
- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- No reference to `evaluate_permission` remains in the tree.
