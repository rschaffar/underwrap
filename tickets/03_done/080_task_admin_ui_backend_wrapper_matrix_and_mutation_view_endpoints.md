# Task: Add backend wrapper, matrix, and mutation-state view endpoints

Parent: `000_story_admin_ui_information_architecture_and_backend_view_models.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Add frontend-facing endpoints for wrapper tables/detail views.
- Add frontend-facing endpoints for access-matrix reads.
- Add read models exposing mutation-window state in a UI-friendly form.
- Keep backend writes authoritative for wrapper mode and policy changes.

## Acceptance criteria

- [ ] Wrapper list/detail endpoints exist for the new UI.
- [ ] Matrix read endpoints exist with filter inputs the UI needs.
- [ ] Mutation state is visible without raw policy-document parsing in the frontend.
- [ ] Tests cover the new endpoint behavior.
