# Task: Remove per-secret mutation windows from the policy model

Parent: `245_story_agent_grant_model_rewrite.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Delete the per-secret unlock-until state machine from `crates/shared-protocol/src/policy.rs`. Identifiers to remove (names may vary): `MutationState`, `UnlockUntil`, `SecretMutationWindow`, and any sibling types.
- Delete `authorize_admin_ui_mutation` (the per-secret mutation-window check) from `crates/backend-backendd/src/server.rs`. Callers are replaced by the global write-lock check from ticket 247.
- Delete the expiry sweeper logic specific to per-secret mutation windows. If the sweeper had no other responsibilities, delete the loop; otherwise simplify.
- Remove the legacy `mutation-as-policy-state.md` design artifact from `tickets/03_done/`. Document the design reversal in this ticket's close note so future readers can find the intent.
- Update `docs/security.md` to remove references to per-secret unlock windows; defer the new grant-model docs to ticket 261.

## Key files likely involved

- `crates/shared-protocol/src/policy.rs`
- `crates/backend-backendd/src/server.rs`
- `crates/frontend-underwrapd/src/` (any per-secret-window code paths)
- `docs/security.md`
- `tickets/03_done/mutation-as-policy-state.md` (delete)

## Acceptance criteria

- [ ] No code in the workspace references per-secret unlock-until state.
- [ ] `cargo build --workspace` is clean after removal.
- [ ] `cargo test --workspace` is clean; obsolete tests deleted or rewritten.
- [ ] `cargo clippy --all-targets --workspace -- -D warnings` is clean (no dead types or unused imports).
- [ ] `tickets/03_done/mutation-as-policy-state.md` deleted.
