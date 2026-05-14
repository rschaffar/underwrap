# Task: Implement secret reveal, binary handling, and mutation edit flow

Parent: `090_story_resource_explorer_secret_reveal_and_mutation_flow.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Add explicit reveal actions for secret values.
- Keep values masked by default and support simple reveal/copy UX.
- Handle UTF-8 vs. binary material safely.
- Support real file upload/download for file-backed secrets with a 1 MiB cap.
- Ensure reveal/download actions are explicitly audited by the backend.
- Implement the operator flow for enable-mutation -> write -> revoke.
- Support delete operations in the new resource UI.

## Handoff notes / current repo state

This is the next major cross-cutting ticket. Relevant groundwork is already in place:

### Frontend already implemented

- Authenticated shell and logout form:
  - `apps/admin-ui/components/shell.tsx`
- Live resource pages:
  - `apps/admin-ui/app/t/[target]/s/[store]/resources/page.tsx`
  - `apps/admin-ui/app/t/[target]/s/[store]/resources/[...resource]/page.tsx`
- Shared resource table component:
  - `apps/admin-ui/components/resource-entry-table.tsx`
- Server-side backend client + view model types:
  - `apps/admin-ui/lib/server/backend-client.ts`
  - `apps/admin-ui/lib/server/ui-types.ts`
  - `apps/admin-ui/lib/server/navigation.ts`

### Backend endpoints already available

- `POST /v1/admin/ui/resource/get`
- `POST /v1/admin/ui/resource/reveal`
- `POST /v1/admin/ui/resource/download`
- `POST /v1/admin/ui/mutation/get`

These are already wired and tested on the backend side.

### What is still missing for this ticket

- frontend reveal UI state and copy UX
- frontend download wiring for file entries
- real file upload flow
- mutation enable/disable controls
- write/update flows for env / libsecret / files
- delete flow in the new UI
- the write-side backend contract the UI should call for mutation changes and delete/write convenience

### Recommended implementation order

1. Extend `apps/admin-ui/lib/server/backend-client.ts` with methods for:
   - mutation state fetch
   - reveal
   - download
   - write/delete calls
2. Add same-origin proxy route handlers / server actions in `apps/admin-ui/app/` for:
   - reveal
   - download
   - upload
   - write
   - delete
   - mutation enable/disable
3. Upgrade `apps/admin-ui/app/t/[target]/s/[store]/resources/[...resource]/page.tsx` to use those actions.
4. Add a small client component for masked reveal/copy state if needed.
5. Add tests and keep `npm test`, `npm run build`, and `nix build .#underwrap-admin-ui-nextjs --no-link` green.

### Open implementation choice to resolve in code

For write-side operations, the next agent can either:

- call the existing low-level admin endpoints already present on the backend, or
- add a small set of dedicated `/v1/admin/ui/*` write helpers for cleaner audited mutation/reveal flows.

Given the greenfield posture, adding cleaner dedicated UI write helpers is acceptable if it improves the first-release design.

## Acceptance criteria

- [ ] Operators can explicitly reveal secret values.
- [ ] Reveals are masked by default, support simple reveal/copy, and never leak into URLs.
- [ ] Binary material has a safe base64/download path.
- [ ] File-backed secrets support real upload/download with a 1 MiB cap.
- [ ] Reveal/download actions are explicitly auditable.
- [ ] Mutation enable/write/revoke is supported end-to-end.
- [ ] Tests cover reveal, upload/download, write, delete, and mutation-gate rejection.
