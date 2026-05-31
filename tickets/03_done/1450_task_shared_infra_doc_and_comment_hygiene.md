# Task: Fix stale docs/comments in shared infra

## Problem

Findings F-11, F-12, F-13, F-18 in
`tickets/_review/shared-infra-ui_2026-05-31.md`:

- `policy-viewer-dialog.tsx:21` — JSDoc claims "plus a quick count of
  writable permissions"; the component renders only a JSON `<pre>` and a
  copy button.
- `policy-viewer-dialog.test.tsx:9` — the `it(...)` description mentions a
  "writable-permissions badge" the body never asserts.
- `policy-viewer-dialog.tsx:33-36` — a comment narrates a completed
  `policy/get → wrapper-permissions/list` migration; there is no `/policy`
  endpoint.
- `use-theme.ts:16` — "No system-preference detection (yet)" reads as a
  deferral marker.

## What to do

1. Remove the writable-count clause from the policy-viewer JSDoc.
2. Rename the test description to what is actually asserted.
3. Rewrite the migration comment in present tense (wrap in `{permissions}`
   so the JSON matches the `PolicyDocument` shape operators recognise).
4. Resolve the `use-theme` "(yet)" marker: reword to state the current
   design, or implement `prefers-color-scheme` (a `matchMedia` read in
   `readInitial`) if desired.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
