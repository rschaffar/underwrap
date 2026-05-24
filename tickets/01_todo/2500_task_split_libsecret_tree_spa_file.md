# Task: Split `libsecret-tree.tsx` into per-concern files

## Background

`web/src/pages/secrets/libsecret-tree.tsx` is currently ~1079 lines
holding four orthogonal concerns:

1. The top-level `LibsecretTree` component (fetches the tree, owns
   the dialog-open state).
2. The `CollectionBlock` + `ItemAttributes` renderers.
3. The `AliasesPanel` (with the dangling-target rule).
4. Six dialog components (create collection, create item, reveal
   item secret, set alias, delete collection, delete item, delete
   alias).

The size makes navigation, focused diffs, and per-component testing
clunky. There's no clean place to colocate a Vitest for just the
`AliasesPanel` dangling-target rule — the current
`libsecret-tree.test.tsx` exercises the whole tree.

## Goal

Move each concern into its own file under
`web/src/pages/secrets/libsecret/`, with the existing top-level
imports unchanged from the call site in `resource-block.tsx`.

## Proposed layout

```
web/src/pages/secrets/libsecret/
├── index.ts                  # re-exports LibsecretTree
├── tree.tsx                  # LibsecretTree component
├── collection-block.tsx      # CollectionBlock + ItemAttributes
├── aliases-panel.tsx         # AliasesPanel (+ dangling-target rule)
├── internal.ts               # ID_PATTERN, parseAttributes,
│                             #   libsecretTreeKey
└── dialogs/
    ├── create-collection.tsx
    ├── create-item.tsx
    ├── reveal-item-secret.tsx
    ├── set-alias.tsx
    ├── delete-collection.tsx
    ├── delete-item.tsx
    └── delete-alias.tsx
```

Update `resource-block.tsx` to import from
`./libsecret` (the index re-export) so the import shape stays one
identifier.

## Scope

### 1. Shared helpers → `libsecret/internal.ts`

Move:
- `ID_PATTERN` regex constant
- `parseAttributes(input: string)` helper
- `libsecretTreeKey(storeName, resourceId)` query-key builder

Each becomes a named export. The `ID_PATTERN` rules-of-shape comment
stays on it.

### 2. Renderers → their own files

`collection-block.tsx`: exports `CollectionBlock` and the small
private `ItemAttributes` inline component. Takes the existing prop
shape.

`aliases-panel.tsx`: exports `AliasesPanel`. The dangling-target
detection (`useMemo`) stays inside it.

### 3. Dialogs → `dialogs/`

One file per dialog. Each exports a single `*Dialog` component.
Cross-dialog imports stay zero: each dialog talks only to the API
client and the local `internal.ts` helpers.

### 4. Tree shell → `tree.tsx`

`LibsecretTree` keeps its current shape: `useQuery` against
`fetchLibsecretTree`, the dialog-open state machine, the empty-state
message. Imports siblings from `./collection-block`,
`./aliases-panel`, and `./dialogs/*`.

### 5. Re-export at `libsecret/index.ts`

`export { LibsecretTree } from "./tree";`

so the only public surface stays one identifier. Update
`resource-block.tsx`:

```diff
-import { LibsecretTree } from "./libsecret-tree";
+import { LibsecretTree } from "./libsecret";
```

### 6. Tests

- Move `libsecret-tree.test.tsx` to `libsecret/tree.test.tsx`
  (whole-tree assertions stay there).
- Add a focused `libsecret/aliases-panel.test.tsx` covering the
  dangling-target rule and the "set alias disabled when no
  collections" UX (currently asserted indirectly via tree tests).
- Existing e2e selectors (`libsecret-collection-*`,
  `libsecret-item-*`, `libsecret-aliases`, dialog roles by
  accessible name) stay unchanged — the split doesn't touch testids
  or the DOM.

## Out of scope

- Adding new dialogs (e.g. an item edit dialog with rotate-secret).
  The structured admin API supports it; the SPA dialog is a separate
  ticket.
- Restructuring the existing `dialogs/` folder under
  `web/src/pages/secrets/dialogs/`. Those (env/files dialogs)
  are not touched by this refactor.

## Acceptance criteria

- [ ] `web/src/pages/secrets/libsecret-tree.tsx` deleted; content
      lives under `libsecret/`.
- [ ] No file under `libsecret/` exceeds ~250 lines.
- [ ] `import { LibsecretTree } from "./libsecret";` is the only
      import statement against the new module from outside.
- [ ] `cd web && npm run build` succeeds.
- [ ] `cd web && npm test` passes; new
      `libsecret/aliases-panel.test.tsx` exercises the dangling
      rule independently.
- [ ] `cd web && npm run test:e2e` exercises the same end-to-end
      flow with no selector changes.
- [ ] No new TypeScript errors with `tsc --noEmit`.
- [ ] No regression in Vitest setup-time (each test file is
      hermetic; no module-cycle blowup from the split).

## Notes for the implementer

- Keep the dialogs purely declarative — no shared hook between
  them. Each dialog owns its own `useForm` and mutation. Sharing
  state across dialogs would force a context provider for marginal
  benefit.
- The `LibsecretTree` component is the only place that holds open-
  dialog state; resist the urge to lift it into each dialog. The
  parent owns the "which dialog is open" state machine.
- Don't relocate testids. The e2e test in
  `web/e2e/secrets.spec.ts` selects on them.
