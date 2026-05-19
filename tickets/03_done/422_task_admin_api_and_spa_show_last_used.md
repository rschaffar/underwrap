# Task: Admin API and SPA expose last_used_at on the Wrappers matrix

Parent: `420_story_last_used_on_pair_records.md`

## Scope

Surface `last_used_at` through the existing admin API and render it
in the Wrappers (matrix) page in the SPA.

### Admin API

The existing pairs listing endpoint (`GET /v1/admin/pairs` or
equivalent, see `crates/backend-backendd/src/server.rs`) now
includes `last_used_at` on each row. No new endpoint required.

`ts-rs` regenerates `WrapperResourcePairRecord` automatically.

### SPA

The Wrappers page (`web/src/pages/wrappers.tsx` per the recent
matrix-to-wrappers rename) renders the field on each pair cell:

- Humanized relative label as primary display, e.g. "3 min ago",
  "yesterday", "never". Use the project's existing relative-time
  helper if any, otherwise introduce one (small, no new dep —
  the SPA already uses `Intl.RelativeTimeFormat`-style fmt
  elsewhere).
- Tooltip on hover shows the absolute ISO-8601 local timestamp.
- "never" rendered in a muted style for pairs that haven't been
  used.
- Cell stays clickable / does not change layout — the addition
  is informational.

The Wrappers page already shows the client-instance display name
header per recent commit `f565133`; this addition is a sibling
detail on the cell body.

### Sorting

Add an optional "sort by last used" affordance on the matrix
header. Default sort unchanged.

## Acceptance criteria

- [ ] Pairs API responses contain `last_used_at`.
- [ ] ts-rs types regenerate cleanly; SPA build green.
- [ ] Wrappers page renders relative time, with tooltip showing
      absolute time, "never" for null.
- [ ] Optional sort-by-last-used is available.
- [ ] Vitest covers the rendering and the "never" case.
- [ ] `cd web && npm run build` and `npm test` green.

## Out of scope

- Per-instance usage history — out of scope of this epic.
- E2E coverage for the matrix interaction — task 423.
