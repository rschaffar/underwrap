# Task: Users admin API and role model

Parent: `240_story_multi_user_with_per_store_access.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

- `GET /v1/admin/users` — admin-only; lists operator records (excluding password hashes from the response).
- `GET /v1/admin/users/{id}` — admin-only.
- `POST /v1/admin/users` — admin-only; creates with `{username, password, role, granted_stores?}`; password hashed with Argon2id server-side.
- `PUT /v1/admin/users/{id}` — admin-only; can change role, grants, username.
- `POST /v1/admin/users/{id}/password` — admin-only path to set any user's password.
- `POST /v1/admin/auth/password` — self-service password change for the current operator (requires current password).
- `DELETE /v1/admin/users/{id}` — admin-only.
- Last-admin protection: cannot delete the last admin or demote them to operator; returns 409 `last_admin_protected`.
- Self-protection: cannot demote yourself if doing so would leave zero admins.

## Acceptance criteria

- [ ] All endpoints implemented with the admin-role guard from ticket 213.
- [ ] Password is never returned in responses, never logged.
- [ ] Last-admin protection enforced and tested with concurrent demotion attempts.
- [ ] Audit events on every mutation include actor and target principal.
- [ ] Tests cover happy paths, last-admin rejection, self-demotion rejection, password not in any response or log, username uniqueness across create/rename.
