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

## Handoff notes / current repo state

Important dependency information:

- backend reveal/download endpoints already emit explicit audit log events:
  - `secret_revealed`
  - `secret_downloaded`
- overview endpoints currently return `recent_audit_events: 0`; there is no
  audit feed endpoint yet.
- This ticket is the place to design the actual queryable audit-history read model.

Suggested implementation direction:

1. Decide where audit data comes from in `backend-backendd` (current tracing-based audit path vs. persisted/queryable feed).
2. Add `/v1/admin/ui/audit/query` and the corresponding `shared-protocol` types.
3. Support the agreed filters: target is frontend-side, backend query should at least cover store/operator/wrapper/resource/action/time-range.
4. Extend tests so reveal/download audit events added earlier are queryable through the new endpoint.

## Acceptance criteria

- [ ] The backend exposes audit-history endpoints suitable for the UI.
- [ ] Supported filters cover target, store, operator, wrapper,
      resource, action, and time range.
- [ ] UI reveal/download actions produce explicit audit events.
- [ ] Tests cover the new audit endpoint behavior and reveal/download
      audit events.
