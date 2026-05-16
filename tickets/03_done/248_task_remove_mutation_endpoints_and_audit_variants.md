# Task: Remove per-secret mutation endpoints and obsolete audit variants

Parent: `245_story_agent_grant_model_rewrite.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Delete the per-secret mutation-window endpoints from the admin API:
  - `/v1/admin/ui/mutation/get` (returned per-secret window state)
  - `/v1/admin/ui/mutation/put` (set the per-secret window)
  - Any sibling `/v1/admin/policy/mutation/...` endpoints
- Keep the **value-write** endpoints (`/v1/admin/ui/resource/put`, `/v1/admin/ui/resource/delete`, etc.). They now gate on the global `writes_enabled` flag from ticket 247, not on a per-secret window.
- Delete audit event variants associated with per-secret mutation-window state changes (`mutation_window_opened`, `mutation_window_closed`, etc.). Keep generic `resource_write` events with operator principal.
- Update or delete corresponding admin UI server-action code in `apps/admin-ui/` that referenced mutation windows. Full Next.js removal lands in 260; this ticket just stops the old UI from compiling against removed endpoints.
- Reconcile the stored audit-history blob format: any persisted events with deleted variants must still deserialize (rename + ignore, or migrate). Document the migration step.

## Key files likely involved

- `crates/backend-backendd/src/server.rs`
- `crates/shared-protocol/src/policy.rs`
- `crates/shared-protocol/src/audit.rs` (or wherever audit variants live)
- `apps/admin-ui/lib/server/...` (transitional cleanup)

## Acceptance criteria

- [ ] Removed endpoints no longer route; integration tests assert 404 on the old paths.
- [ ] Resource-value writes succeed when `writes_enabled` is true; fail with 403 `writes_globally_disabled` otherwise.
- [ ] Audit history continues to load: obsolete variants either renamed to a tombstone variant or dropped with a documented migration.
- [ ] No reference to mutation-window endpoints remains in the workspace.
