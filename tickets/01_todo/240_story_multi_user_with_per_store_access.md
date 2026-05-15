# Story: Multi-user with per-store access grants

Parent: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Background

The system is single-operator today. The new design supports multiple operators with two roles (`admin`, `operator`) and per-store access grants.

- Admin can do anything, including create/delete operators and stores.
- Operator can only access stores they've been granted.
- All access decisions live server-side; the UI hiding controls is UX, not security.

## Tasks

1. `240_task_users_admin_api_crud_and_role_model.md`
2. `241_task_per_store_access_grants_and_filtering.md`
3. `242_task_web_users_page_and_grant_editor.md`
4. `243_task_multi_user_test_coverage.md`

## Acceptance criteria

- [ ] Admin can create/edit/delete operators and toggle role + grants.
- [ ] Operator-role accounts see only granted stores in every list/read/write surface.
- [ ] Last-admin protection: cannot delete or demote the last admin.
- [ ] Audit events include the operator principal on every mutation.
- [ ] Tests cover authz negatives at every layer (Rust unit + integration, Vitest, Playwright, VM).
