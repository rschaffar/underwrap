# Task: Remove dead code and micro-simplifications in shared infra

## Problem

Findings F-06, F-09, F-14, F-16, F-17 in
`tickets/_review/shared-infra-ui_2026-05-31.md`:

- `api/client.ts:119-122` — dead SSR base-URL branch (`typeof window …`);
  no SSR pass exists.
- `app-shell.tsx:38` (`NAV`) and `api/client.ts:142` (`unwrap`) are
  exported with no external consumers.
- `api/client.test.ts:141-144` — "exposes the raw openapi-fetch client"
  asserts only the third-party library shape.
- `app-shell.tsx:79-81` — `cn()` called with a single static string.
- `policy-viewer-dialog.tsx:47` (and `reveal-secret-view.tsx:73`) —
  `window.setTimeout` should be plain `setTimeout`.

## What to do

1. Replace the base-URL ternary with `const apiBaseUrl = window.location.origin;`
   and drop the dead-branch comment.
2. Drop the `export` on `NAV` and `unwrap`.
3. Delete the library-only `client.test.ts` case (and unexport `client` if
   nothing else needs it).
4. Replace the single-arg `cn(...)` with a plain `className="…"` string.
5. Use plain `setTimeout(...)` in `policy-viewer-dialog.tsx` and
   `reveal-secret-view.tsx`.
6. Final category-7a re-read of the touched files.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
