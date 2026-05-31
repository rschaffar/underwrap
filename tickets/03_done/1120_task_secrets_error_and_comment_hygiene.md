# Task: Fix reveal error message, libsecret id length, and comment hygiene

## Problem

Findings F-05, F-09, F-10, F-11, F-12 in
`tickets/_review/secrets-libsecret-ui_2026-05-31.md`:

- `reveal-secret-view.tsx:104-111` — the "could not be decoded as UTF-8"
  message misrepresents `atob()` (Latin-1 base64 decode, not UTF-8); the
  catch only fires on invalid base64.
- `libsecret/helpers.ts:3-6` — the comment documents a `max 128` rule that
  `libsecretIdField()` / `optionalLibsecretIdField()` do not enforce.
- `reveal-secret-view.tsx:51-56` — `gcTime: 0` collects the cache on
  unmount but the secret transiently lives in the JS heap with no
  zero-fill; this boundary is undocumented.
- `resource-block.tsx:43-60` — WHAT-comments on `summary` / `displayLabel`.
- `libsecret/collection-block.tsx:132-135` — WHAT-comment on attribute
  rendering.

## What to do

1. Reword the reveal decode error to name a server-side base64 problem
   (the value could not be base64-decoded).
2. Add `.max(128, …)` to both libsecret id validators in `helpers.ts` so
   the client enforces what the comment claims (closes the gap; no valid
   input changes).
3. Add a short comment at `reveal-secret-view.tsx:51-56` stating the
   in-memory-secret lifetime / no-zero-fill model.
4. Delete the `displayLabel` WHAT-comment; trim the `summary` comment to
   the one-sentence "why" (libsecret content is counted by the tree).
5. Trim the `collection-block.tsx` comment to the "list is normally short"
   rationale only.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
