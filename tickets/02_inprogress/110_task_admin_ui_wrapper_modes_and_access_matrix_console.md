# Task: Build the wrapper-mode console and modern access-matrix UI

Parent: `110_story_wrapper_modes_and_access_matrix_console.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Build a filterable wrappers table and detail/edit flow within the multi-target admin console.
- Support locked, unlocked, unlocked-until, and per-call-approval modes.
- Replace raw epoch editing with real date/time UX.
- Build a clearer access-matrix UI with filtering and toggles.

## Handoff notes / current repo state

Backend read-model support for this ticket is already present:

- `POST /v1/admin/ui/wrappers/query`
- `POST /v1/admin/ui/matrix/query`
- `POST /v1/admin/ui/mutation/get`

Those endpoints were added in backend task `080` and are covered by backend tests.

Frontend route placeholders already exist:

- `apps/admin-ui/app/t/[target]/s/[store]/wrappers/page.tsx`
- resource detail sidebar in `apps/admin-ui/app/t/[target]/s/[store]/resources/[...resource]/page.tsx`

Likely implementation plan:

1. Extend `apps/admin-ui/lib/server/backend-client.ts` with wrapper and matrix query methods.
2. Replace the placeholder wrappers page with live data.
3. Add edit flows for wrapper mode changes.
4. Add a real matrix table/filter UX.
5. Reuse the same session/CSRF/same-origin infrastructure already implemented in tasks `040` and `050`.

## Acceptance criteria

- [ ] Operators can browse and edit wrapper modes in a modern UI.
- [ ] Unlock-until uses a proper date/time interaction.
- [ ] Per-call approval key editing is supported.
- [ ] The access matrix is filterable and easier to understand than today.
- [ ] Tests cover wrapper mode edits and matrix toggles.
