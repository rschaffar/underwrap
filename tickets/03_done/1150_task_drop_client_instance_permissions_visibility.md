# Task: Make policy cleanup visibility-correct for locked stores

Resolves: B-ADM-20.

## Problem

`crates/backend-backendd/src/admin_api/policy.rs:34-53` —
`drop_client_instance_permissions` does:

```rust
let store = match state.stores.read().unwrap().unlocked_backend(store_id) {
    Ok(s) => s,
    Err(_) => return Ok(()),
};
```

Both `RegistryLookupError::Locked` and `RegistryLookupError::NotFound`
flow into `Ok(())`. When a store is **locked** at the moment an
instance is remapped or deleted, orphaned permissions stay on disk
silently — no warning, no audit line. This contradicts the docstring
claim that the function exists "so authorizations don't silently
survive across the reassignment."

Callers in `crates/backend-backendd/src/admin_api/client_instances.rs:343-357,410-421`
log "best-effort" warnings only when the cascade *fails after the
lookup succeeded* — the locked path produces no signal whatsoever.

## Security posture

This ticket strengthens an audit-trail invariant. It does not weaken
any boundary in `CLAUDE.md` "Security-critical rules" or `docs/security.md`.

## What to do

1. Match on `RegistryLookupError` explicitly in
   `drop_client_instance_permissions`:
   - `Locked` → emit a structured `tracing::warn!(store = %store_id,
     ...)` and a `log_admin_event` audit line. Consider returning an
     error so the caller can decide whether to refuse the parent
     operation. At minimum the warning + audit must fire so operators
     can find the orphan rows.
   - `NotFound` → `tracing::info!` + `Ok(())` is fine.
2. Update the docstring to describe the actual behaviour on each
   `RegistryLookupError` arm.
3. In the callers in `client_instances.rs:343-357,410-421`, decide
   whether the locked-store case should refuse the remap/delete or
   proceed with the warning. Document the choice in code comments
   (a WHY-comment is appropriate here per the rules).
4. Add an integration test (`cargo test -p integration-tests`) that
   asserts:
   - Remapping a client instance with a locked store produces a
     warning + audit event referencing the orphan permissions.
   - Deleting a client instance with a locked store does the same.

## Acceptance

- `cargo test -p backend-backendd`
- `cargo test -p integration-tests`
- `nix build .#checks.x86_64-linux.vm-test`
- `cargo clippy --all-targets --workspace -- -D warnings`
