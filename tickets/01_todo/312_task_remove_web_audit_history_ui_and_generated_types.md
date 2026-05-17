# Task: Remove web audit-history UI and generated types

Parent: `310_story_remove_product_audit_log.md`

## Scope

Remove the frontend/admin-SPA surface for persisted audit history:

- delete audit-history routes/pages/components
- remove audit-history API client calls
- remove generated TS types for audit-history request/response models
- remove UI wording that references an audit-history console

## Acceptance criteria

- [ ] Audit-history pages/routes/components are removed.
- [ ] Audit-history API client calls are removed.
- [ ] Audit-history TS types are removed from generated/exported frontend type surfaces.
- [ ] UI copy no longer promises a persisted audit-history feature.
