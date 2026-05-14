# Task: Build the read-only, filterable audit-history console

Parent: `115_story_admin_ui_audit_history_console.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Build the audit/history page in the new admin UI.
- Add filters for target, store, operator, wrapper, resource, action,
  and time range.
- Present the feed in a way that fits the dark, high-contrast ops
  console design.
- Keep the page read-only in v1.

## Handoff notes / current repo state

Frontend route placeholder already exists:

- `apps/admin-ui/app/t/[target]/s/[store]/audit/page.tsx`

The shell/navigation already includes Audit as a first-class section. This ticket should replace the placeholder table and fake filters with live data from the backend audit endpoint added by task `115`.

Suggested implementation direction:

1. Extend `apps/admin-ui/lib/server/backend-client.ts` with an audit query method.
2. Replace placeholder filters with real query params / form controls.
3. Keep the page read-only.
4. Preserve the dark high-contrast console styling already established in the shell.

## Acceptance criteria

- [ ] Operators can browse audit history from the new UI.
- [ ] The main audit filters are available and usable.
- [ ] The UI clearly surfaces reveal/download events.
- [ ] Tests cover the audit page and filtering flow.
