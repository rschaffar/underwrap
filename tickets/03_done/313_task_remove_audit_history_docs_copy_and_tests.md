# Task: Remove audit-history docs, copy, and tests

Parent: `310_story_remove_product_audit_log.md`

## Scope

Clean up project references to the removed product audit-history feature:

- update docs that describe an audit-history console or persisted audit events
- update tests that assert backend-stored audit history
- keep documentation aligned with the simplified "journald/stdout logging only" model

## Acceptance criteria

- [ ] Docs no longer describe a product audit-history console.
- [ ] Tests no longer expect backend-stored audit-history behavior.
- [ ] Remaining text clearly points to normal server logs instead.
