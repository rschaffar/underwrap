# Task: Make `LibsecretTree.enabled` explicit (no auto-fetch default)

## Problem

Finding F-07 in `tickets/_review/secrets-libsecret-ui_2026-05-31.md`.

`libsecret/tree.tsx:24-34` defaults `enabled = true`. The prop exists to
defer the libsecret fetch until the parent `<details>` is expanded, so a
caller that forgets it fetches libsecret data for every visible resource
on mount — exactly what the prop is meant to prevent. The two tree tests
omit `enabled`, so the guarded (no-fetch) path is never covered.

The sole production caller, `ResourceBlock`, always passes `enabled`
explicitly, so production behaviour does not change.

## What to do

1. Make `enabled` required (or default it to `false`) so the fetch only
   fires when the caller opts in.
2. Update the two `tree.test.tsx` cases to pass `enabled` explicitly and
   add a case asserting that `enabled={false}` performs **no**
   `fetchLibsecretTree` call.

## Acceptance

- `cd web && npm test` (incl. the new guarded-path case)
- `cd web && npm run build`
