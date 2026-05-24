# Story: Admin SPA — libsecret tree view

## Background

See `epic_libsecret_structured_admin.md`. The admin SPA today renders
the libsecret slot as a generic flat-KV list, exposing the synthetic
`_aliases` / `<col>/_meta` / `<item>.meta` / `<item>.secret` rows. Once
`2200_story_libsecret_structured_admin_api.md` lands, the SPA should
switch to the structured endpoints and render a real tree.

The SPA lives outside this repository tree, wired in at build time
through `$UNDERWRAP_WEB_DIST` and embedded into `underwrap-backendd`
via `rust-embed`. This story is tracked here so the rollout order is
visible.

## Goal

On the resource detail page, libsecret is rendered as:

- An **Aliases** panel: a small table of alias name → collection.
  Operator can add, edit, delete alias rows.
- A **Collections** section: a list (or expandable tree) of
  collections. Each collection shows its label and its items.
  Operator can add / edit / delete collections.
- Inside a collection, items appear with label, content type, attribute
  table, created / modified timestamps. Operator can add / edit / delete
  items and reveal the secret on demand.

Synthetic flat rows (`_aliases`, `<col>/_meta`, `<item>.meta`,
`<item>.secret`) never appear in the UI.

## Scope

- Replace the generic libsecret KV listing with a tree view backed by
  `GET .../libsecret`.
- Add a small dedicated "Reveal secret" action for items, calling
  `GET .../libsecret/collections/{cid}/items/{iid}/secret`. The secret
  value is shown on operator request and never auto-loaded with the
  tree.
- Wire the existing add / edit / delete affordances to the new
  structured POST / PATCH / DELETE endpoints.
- Surface server-side validation errors (id shape, leading `_`,
  unknown collection on alias PUT) inline next to the relevant field.
- Make it clear in the collections list which collection id matches
  each alias (highlight the row pointed at by the `default` alias).
- Remove any existing UI code that constructed the flat-KV id pattern.

## Out of scope

- Locked / unlocked UI (the service is always unlocked).
- Per-collection "set as default" toggle in the UI; aliases are managed
  through the explicit Aliases panel.
- Bulk import / export.

## Acceptance criteria

- [ ] Resource page renders the libsecret tree from the structured
      endpoint.
- [ ] All operator actions go through the structured endpoints; no
      requests to `.../secrets/.../libsecret/<key>` flat paths.
- [ ] Item secret bytes are fetched only on explicit "Reveal" /
      "Edit secret" actions.
- [ ] Existing Vitest / Playwright suites updated and green.
- [ ] Manual smoke test against a real `underwrap-backendd` confirms
      every CRUD action lands the expected change in the resource blob.

## Notes for the implementer

- Until `2300` lands, the flat libsecret endpoints still exist; once
  the SPA stops calling them, they can be retired.
- Treat the tree view as authoritative — do not maintain a separate
  flat list alongside it.
- Reuse the existing audit-log viewer; the new event names from
  `2200` should render meaningfully without UI changes.
