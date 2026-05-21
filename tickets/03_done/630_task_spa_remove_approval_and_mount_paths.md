# Task: SPA — drop per-call approval UI and mount-paths display

Parent: `600_story_runtime_api_redesign.md`

## Scope

Remove every SPA element that exists only because `PerCallApproval`
mode or `files_mount_paths` existed. Replace the Secrets browser's
mount-path grouping with grouping derived from the file-section keys
in the resource blob.

### Wrapper permissions page

- `web/src/pages/wrapper-permissions.tsx`:
  - Drop the per-call-approval mode option from the mode selector.
  - Drop the "approval key" input field that pairs with that mode.
  - Drop any `approval_key` / `PerCallApproval` references in the
    cell rendering, the edit dialog, and the form validation.
  - The mode dropdown becomes `Locked | Unlocked | UnlockedUntil`.
- `web/src/components/wrapper-permission-*`: drop the approval-key
  preview, the approval-key copy-to-clipboard helper, the
  approval-key tooltip.
- TypeScript bindings (`web/src/api/types.ts` or wherever ts-rs
  exports land): the `WrapperApprovalMode` union has only three
  variants after `610_task_*` ships.

### Secrets browser

- `web/src/pages/secrets.tsx` (and component files used there):
  - Drop the `<mount>/<inner>` rendering for file entries.
  - Group the file section's entries by the first path component of
    each key (e.g. `.config/gh/` and `.config/git/` for a resource
    whose file section has those prefixes).
  - The grouping is purely a display nicety; no backend data drives
    it. Sort sections alphabetically.

### Client instance detail / pair detail

- Drop the `files_mount_paths` field from any pair-row rendering.
- Verify that the wrapper-resource pair card doesn't show "mount paths"
  anywhere.

### Test updates

- `web/src/**.test.tsx`: every test that asserts on the per-call
  approval UI or on the mount-paths display fails after the schema
  drops the fields. Update them to assert the new rendering.
- `web/playwright/*.spec.ts`: e2e specs that exercise the approval
  selector or the mount-paths column get rewritten to assert the new
  shape.
- The `Vitest` golden snapshots for the wrapper-permissions page need
  re-baselining.

### Acceptance criteria

- [ ] `grep -rn "PerCallApproval\|approval_key\|files_mount_paths\|mount_path" web/src/` returns no hits in production code.
- [ ] `cd web && npm test` green.
- [ ] `cd web && npm run test:e2e` green (run inside `nix develop`).
- [ ] The Secrets browser still groups files visually, but the
      grouping comes from the keys, not from a backend-provided
      mount-paths list.
- [ ] The wrapper-permissions mode dropdown shows three options.

### Out of scope

- Repositioning / restyling the wrapper-permissions matrix beyond
  what falls out of the field deletions.
- Bringing back any approval-shaped feature through different
  client-side mechanics.
