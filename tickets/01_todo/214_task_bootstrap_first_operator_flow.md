# Task: First-admin bootstrap flow

Parent: `210_story_rust_native_auth_sessions_multiuser.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- `GET /v1/admin/auth/bootstrap/status` — returns `{ bootstrap_required: bool }` based on whether the operator collection is empty and a persistent bootstrap-completed sentinel is unset.
- `POST /v1/admin/auth/bootstrap` — accepts `{username, password}`:
  - 409 `bootstrap_already_completed` if the sentinel is set
  - 409 `operators_already_exist` if any operator exists
  - On success: creates the first admin, sets the sentinel, immediately issues a session, and returns it.
- SPA shows a Bootstrap page if status reports `bootstrap_required`; falls back to login otherwise.
- The sentinel is persistent: once bootstrap has succeeded, the endpoint is locked even if all operators are later deleted. This prevents a hostile reset from creating an unprivileged-becomes-admin escalation.
- Offline recovery: a CLI subcommand `underwrap-backendd admin recover-operator` lets an operator with disk access create an admin directly on the store (documented as physical-access-required). This is the fix for the locked-out case.
- Every bootstrap attempt audited.

## Key files likely involved

- `crates/backend-backendd/src/auth.rs`
- `crates/backend-backendd/src/operators.rs`
- `crates/backend-backendd/src/admin_cli.rs`

## Acceptance criteria

- [ ] Bootstrap returns 409 after the first success (sentinel persisted).
- [ ] Bootstrap creates an admin-role operator and issues a session in one call.
- [ ] Race test: concurrent bootstrap calls; exactly one succeeds.
- [ ] Audit event emitted on success and on each failed attempt (with reason).
- [ ] `underwrap-backendd admin recover-operator` works against a backend that is not running (operates directly on the store on disk).
- [ ] Tests cover: empty-store success, second-call rejection, replay, audit emission, CLI recovery happy path.
