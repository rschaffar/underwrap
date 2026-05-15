# Task: Per-store access grants and server-side filtering

Parent: `240_story_multi_user_with_per_store_access.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

Every store-scoped admin endpoint consults the caller's `granted_stores` before responding:

- `/v1/admin/ui/overview` — only includes granted stores' counts
- `/v1/admin/ui/resources/query` — 404 on ungranted store
- `/v1/admin/ui/resource/get|reveal|download|put|delete` — 404 on ungranted
- `/v1/admin/ui/mutation/get|put` — 404 on ungranted
- `/v1/admin/ui/wrappers/query` — 404 on ungranted
- `/v1/admin/ui/matrix/query` — 404 on ungranted (matrix tab loads; empty state shown if no grants)
- `/v1/admin/policy/...` — 404 on ungranted
- `/v1/admin/stores` — list filtered; CRUD admin-only

Admin role bypasses the filter (sees everything). 404 (not 403) on ungranted access to avoid leaking store existence.

## Acceptance criteria

- [ ] All store-scoped endpoints filter by grants; admin bypasses.
- [ ] Tests cover at least one positive and one negative case per endpoint.
- [ ] No cross-store leak through any endpoint, verified by a comprehensive negative-test sweep that iterates every endpoint with an ungranted store name.
- [ ] Audit events for ungranted-store access attempts emit a `denied_grant` reason (so admins can detect probing).
