# Story: Remove the product audit-log feature

Parent: `epic_remove_product_audit_log.md`

## Background

The backend currently persists audit-history events into backend storage and exposes them through admin UI/API surfaces. We no longer want that feature. Journald/stdout logging is sufficient for the first release.

## Desired design

- Remove the reserved backend audit-history secret and all code paths that write to it.
- Remove audit-history query endpoints and protocol types.
- Remove audit-history pages/routes/generated types from the SPA.
- Keep structured runtime logs, even if some existing helper/module names still say `audit` initially.
- Remove product/UI wording that implies persisted audit-history exists.

## Acceptance criteria

- [ ] Backend no longer reads/writes `__underwrap/admin/ui/audit-history`.
- [ ] `/v1/admin/ui/audit/query` is removed.
- [ ] Backend overview/state models no longer expose audit-history counters.
- [ ] Audit-history frontend code and generated TS types are removed.
- [ ] Tests no longer assert persisted audit-history behavior.
- [ ] Docs describe journald/stdout logging, not an in-product audit-history console.
