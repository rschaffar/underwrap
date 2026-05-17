# Task: Remove backend audit-history storage and API

Parent: `310_story_remove_product_audit_log.md`

## Scope

Remove the backend-side persisted audit-history feature:

- stop reading/writing `__underwrap/admin/ui/audit-history`
- remove helper functions and state related to stored audit history
- remove `/v1/admin/ui/audit/query`
- remove audit-history counters from backend overview/view models
- keep normal structured journald/stdout logging

## Acceptance criteria

- [ ] Backend no longer persists audit-history events.
- [ ] Audit-history query endpoint is removed.
- [ ] Backend overview models no longer expose audit-history counters.
- [ ] Structured runtime logs still exist.
