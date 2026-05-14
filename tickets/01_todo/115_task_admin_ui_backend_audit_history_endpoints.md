# Task: Add backend audit-history endpoints for the new UI

Parent: `115_story_admin_ui_audit_history_console.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Add frontend-facing audit-history endpoints/read models for the new UI.
- Support filtering by target, store, operator, wrapper, resource,
  action, and time range.
- Ensure UI reveal/download actions are emitted as explicit backend
  audit events and can be queried through the audit feed.
- Shape the responses for direct frontend consumption.

## Acceptance criteria

- [ ] The backend exposes audit-history endpoints suitable for the UI.
- [ ] Supported filters cover target, store, operator, wrapper,
      resource, action, and time range.
- [ ] UI reveal/download actions produce explicit audit events.
- [ ] Tests cover the new audit endpoint behavior and reveal/download
      audit events.
