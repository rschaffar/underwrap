# Task: Add backend overview and resource-index endpoints for the new UI

Parent: `000_story_admin_ui_information_architecture_and_backend_view_models.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Add frontend-oriented admin API endpoints for:
  - multi-target/backend/store overview
  - resource listing/search
  - high-level counts/status needed by the UI shell
- Prefer a dedicated UI namespace instead of overloading the current
  low-level endpoints.

## Acceptance criteria

- [ ] The backend exposes overview/read-model endpoints for the UI.
- [ ] Resource listing supports filtering/search inputs needed by the UI.
- [ ] Responses are shaped for direct frontend consumption.
- [ ] Tests cover the new endpoint behavior.
