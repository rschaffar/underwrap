# Task: Stores CRUD admin API

Parent: `230_story_stores_management_declarative_and_dynamic.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- `GET /v1/admin/stores` — list (admin: all; operator: only granted, via the per-store filter from ticket 241).
- `GET /v1/admin/stores/{name}` — detail (admin: all; operator: only granted).
- `POST /v1/admin/stores` — admin-only. Body: `{name, backend: "plain"|"encrypted", dir, description?}`. Server writes `stores.d/<name>.yaml` atomically to the dynamic dir, validates that `dir` is under a configured root path, optionally creates the directory with mode 0700.
- `PUT /v1/admin/stores/{name}` — admin-only. Update description; rotate encrypted-store key (separate endpoint in ticket 234). Backend type and dir are immutable post-creation.
- `DELETE /v1/admin/stores/{name}` — admin-only. Refuse if the store has any secrets unless `?force=true` is set; refuse on declarative stores; audit. `force=true` does not delete the on-disk store dir or key file by default — emit a follow-up cleanup instruction in the response.
- All mutations on declarative stores return 409 with code `declarative_immutable` and a `source_path` hint.
- All admin store actions are audited with operator principal.

## Acceptance criteria

- [ ] CRUD endpoints implemented with role + grant guards from ticket 213.
- [ ] Declarative-store mutations rejected with the documented error code.
- [ ] Atomic writes via `tempfile::NamedTempFile::persist()`.
- [ ] `dir` validation enforces a configured allowed-root prefix; absolute paths outside the allowed root rejected.
- [ ] Tests cover all paths: create, list, get, update-description, delete (empty + force-non-empty), declarative rejection, dir validation, non-admin denied, audit emission.
