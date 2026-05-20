# Task: Rename SPA Wrappers page → Wrapper permissions

Parent: `440_story_rename_to_wrapper_permissions.md`
Depends on: `441_task_rename_rust_protocol_types_and_backend.md`
(needs the regenerated `ts-rs` types and renamed endpoint paths).

## Scope

### Regenerated types

- After 441 lands, run the `ts_export` test so `web/src/api/types/*.ts`
  regenerates. Delete the stale files:
  - `web/src/api/types/AgentGrant.ts`
  - `web/src/api/types/AdminAgentGrantsListRequest.ts`
  - `web/src/api/types/AdminAgentGrantsListResponse.ts`
  - `web/src/api/types/AdminAgentGrantUpsertRequest.ts`
  - `web/src/api/types/AdminAgentGrantDeleteRequest.ts`
- Update `web/src/api/types/index.ts` re-exports to the new names
  (`WrapperPermission`, `AdminWrapperPermissions*`).

### API client

- `web/src/api/client.ts`:
  - Rename `postGrantsList` → `postWrapperPermissionsList`,
    `postGrantUpsert` → `postWrapperPermissionUpsert`,
    `postGrantDelete` → `postWrapperPermissionDelete`.
  - Update endpoint URL strings to
    `/v1/admin/wrapper-permissions/{list,upsert,delete}`.
  - Update the "Agent grants (ticket 246)" comment.

### Page rename

- File rename: `web/src/pages/wrappers.tsx` →
  `web/src/pages/wrapper-permissions.tsx` (also `.test.tsx`).
- Rename the `APP_ROUTE_PATHS` constant key for this route to
  something like `WRAPPER_PERMISSIONS`. **Keep the path string
  `/wrappers` unchanged** — only the internal constant name changes.
- `web/src/router.tsx`: update the import and `createRoute` reference.
- `web/src/components/app-shell.tsx` line 43: nav label
  `"Wrappers"` → `"Wrapper permissions"`.

### User-facing copy + identifiers (`wrapper-permissions.tsx`)

- Document title: `underwrap — wrappers` →
  `underwrap — wrapper permissions`.
- `<h2>` heading: `Wrappers` → `Wrapper permissions`.
- Description (currently lines 256–259): rewrite so it does not
  mention "agent grant". Suggested copy: *"Wrapper/resource pairs
  registered with the backend, and the permission each runtime client
  instance holds against them."*
- Identifier renames (mechanical, follow the regenerated types):
  - `grantIndex` → `permissionIndex`
  - `grants: AgentGrant[]` → `permissions: WrapperPermission[]`
  - dialog state `existing: AgentGrant | null` →
    `existing: WrapperPermission | null`
  - `CellContent({ grant })` → `CellContent({ permission })`
- Dialog refusal message (line ~815): "Clear those grants on the
  matrix above, then try again." → "Clear those permissions in the
  table above, then try again."
- Leave the ssh-agent comment at line ~976 alone (it refers to the
  signing protocol, not the policy model).

### Other components

- `web/src/components/policy-viewer-dialog.tsx`:
  - Comment lines 27–28: "The Matrix page" → "The Wrapper permissions
    page".
  - Comment line 32: "Editing intentionally stays in the Matrix view."
    → "Editing intentionally stays in the Wrapper permissions view."
  - Help text line 82: "Read-only — use the Matrix view to change
    grants." → "Read-only — use the Wrapper permissions view to
    change permissions."
- `web/src/pages/secrets.tsx` line 140 comment: "same shape as the
  Matrix page." → "same shape as the Wrapper permissions page."

### Tests

- `web/src/pages/wrapper-permissions.test.tsx`: update imports, test
  descriptions, and any asserted copy.
- `web/tests/e2e/` (Playwright): grep for "matrix", "Wrappers",
  "Agent grant", or `/grants` URL fragments and update.

## Verification

- [ ] `cd web && npm test` — vitest green.
- [ ] `nix develop -c bash -c 'cd web && npm run test:e2e'` —
      playwright green.
- [ ] `nix build .#checks.x86_64-linux.vm-test` — VM test still
      passes (admin SPA end-to-end check).
- [ ] Smoke test: launch backend, log in, confirm nav reads
      "Wrapper permissions", page heading + title match, edit /
      delete flows still work.
- [ ] `rg -i 'matrix|agent[- _]?grant' web/` returns nothing.

## Out of scope

- Documentation — covered by 443.
- Renaming the URL path `/wrappers` — explicitly out (decision in
  parent story).
