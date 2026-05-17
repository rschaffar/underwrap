# Epic: Remove the product audit-log feature

## Background

underwrap currently has two different things under the word "audit":

- ordinary structured runtime logs to journald/stdout
- a persisted backend-stored audit-history feature exposed through the admin API and UI

For the first release we want to simplify this aggressively. The persisted/queryable audit-history feature adds backend state, API surface, UI surface, test burden, and conceptual weight that we do not currently need.

This project is still greenfield. We should prefer a cleaner first-release design over preserving an early feature just because it already exists.

## Product decision

- Remove the backend-stored audit-history feature entirely.
- Keep ordinary structured logs to journald/stdout on `underwrapd` and `underwrap-backendd`.
- Deletion means deletion: no retained audit-history store, no audit-history UI, no audit-history query API.

## Goals

1. Remove persisted audit-history storage from the backend.
2. Remove audit-history API endpoints and protocol types.
3. Remove audit-history UI/routes/copy.
4. Keep normal structured server logs for operational visibility.
5. Clean up tests and docs so they no longer describe an in-product audit log.

## Out of scope

- Redesigning normal runtime/server logging.
- The larger runtime-client/store/enrollment redesign tracked separately.

## Story

1. `310_story_remove_product_audit_log.md`

## Acceptance criteria

- [ ] `__underwrap/admin/ui/audit-history` is no longer read or written by the backend.
- [ ] Audit-history admin API endpoints and request/response types are removed.
- [ ] Audit-history UI pages, routes, and generated TS types are removed.
- [ ] Current-state UI copy no longer promises persisted audit events.
- [ ] Existing structured logs to journald/stdout remain in place.
- [ ] Tests and docs are updated for the simplified model.
