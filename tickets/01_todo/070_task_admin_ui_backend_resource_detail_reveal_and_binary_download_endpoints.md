# Task: Add backend resource-detail, reveal, and binary-download endpoints

Parent: `000_story_admin_ui_information_architecture_and_backend_view_models.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Add frontend-oriented endpoints for resource detail views.
- Add explicit reveal endpoints for env/files/libsecret entries.
- Define safe handling for binary / non-UTF-8 material.
- Add a download/base64 path where text rendering is inappropriate.

## Acceptance criteria

- [ ] Resource detail endpoint returns slot-oriented data for the UI.
- [ ] Secret reveal is explicit rather than embedded in list responses.
- [ ] Binary values are handled safely without assuming UTF-8.
- [ ] Tests cover reveal and binary response behavior.
