# Task: Write the admin UI RFC with page map and endpoint contract

Parent: `000_story_admin_ui_information_architecture_and_backend_view_models.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Write a design doc in `docs/` for the new Next.js admin UI.
- Define the page map, primary operator workflows, and navigation model,
  including first-class multi-admin target switching and an audit page.
- Define the frontend-facing backend contract for overview, resource
  index, resource detail, reveal, upload/download, wrappers, matrix,
  audit history, and mutation state.
- Define text vs. binary secret reveal semantics plus the 1 MiB file
  upload/download limit and reveal/download audit behavior.

## Acceptance criteria

- [ ] A new design doc exists in `docs/`.
- [ ] The page map and major workflows are explicit.
- [ ] Required backend endpoints and payloads are listed.
- [ ] Reveal/upload/download semantics are defined for UTF-8 and binary
      values, including the 1 MiB cap and audit expectations.
