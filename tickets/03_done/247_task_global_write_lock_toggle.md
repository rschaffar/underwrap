# Task: Global write-lock toggle

Parent: `245_story_agent_grant_model_rewrite.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- Add a single setting per store: `writes_enabled: bool`. **Default: `false`** (writes off out of the box; admin opts in deliberately).
- Persisted in the policy document under a reserved field, or as a sibling reserved secret id (e.g. `__underwrap/admin/writes-enabled`). Pick in implementation.
- Backend gates every write path on this flag:
  - Wrapper-agent writes via the daemon channel (libsecret mode and any future direct writes)
  - Admin API write endpoints that survive ticket 248 (`/v1/admin/ui/resource/put`, `/v1/admin/ui/resource/delete`, store CRUD that mutates secret data)
- Endpoints:
  - `GET /v1/admin/policy/writes-enabled` — returns `{enabled: bool, store: <name>, last_changed_at: <epoch>, last_changed_by: <username>}`
  - `PUT /v1/admin/policy/writes-enabled` — body `{enabled: bool}`. Admin-only. Audited (actor + new state + previous state).
- When writes are disabled, write attempts return **403** with a stable code `writes_globally_disabled`.
- UI surface (delivered by ticket 250):
  - Prominent toggle at the top of the Matrix tab and a mirror in Settings.
  - Banner when writes are enabled: "Writes enabled globally. Disable when finished."

## Out of scope for v1

- Auto-disable after a duration. Manual toggle is enough; revisit later.
- Per-grant or per-secret write ACL. Explicitly out of scope.

## Key files likely involved

- `crates/shared-protocol/src/policy.rs`
- `crates/backend-backendd/src/server.rs`
- `crates/frontend-underwrapd/src/wrapper_flow.rs`

## Acceptance criteria

- [ ] `writes_enabled` defaults to `false` on a fresh store.
- [ ] All write paths (admin UI + wrapper agents) check the flag.
- [ ] Admin-only endpoint to toggle the flag; audited on every change.
- [ ] Disabled-writes attempts return 403 with `writes_globally_disabled`.
- [ ] Tests cover: default off, enable + write + disable + write-rejected, audit emission, wrapper write blocked when disabled, admin-UI write blocked when disabled.
