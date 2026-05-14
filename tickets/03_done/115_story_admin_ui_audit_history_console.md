# Story: Build the read-only audit history console

Epic: `tickets/01_todo/epic_nextjs_admin_ui_replatform.md`

## Background

A polished first-release operator console should let operators inspect
what happened, not just mutate state. The new UI should therefore ship
with a read-only, filterable audit/history page backed by explicit
backend audit events, including UI-driven reveal and download actions.

## Scope

- Add the backend read models needed for a UI-friendly audit/history
  feed.
- Build a read-only audit page in the new admin console.
- Support filtering by target, store, operator, wrapper, resource,
  action, and time range.
- Ensure reveal/download actions appear as explicit audit events.

## Acceptance criteria

- [ ] Operators can open a read-only audit/history page in the new UI.
- [ ] Audit history is filterable by the main operator dimensions.
- [ ] UI reveal/download actions are visible as explicit audit events.
- [ ] The audit page feels like a first-class part of the console, not
      an afterthought.

## Child tasks

- `115_task_admin_ui_backend_audit_history_endpoints.md`
- `116_task_admin_ui_audit_history_console.md`
