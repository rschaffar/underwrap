# Task: Collapse triple-sort and inline epoch coercion to shared helpers

## Background

Two small simplifications in the secrets page area:

- `web/src/pages/secrets/model.ts:51-55` runs three identical
  `localeCompare` sorts, one per slot. The file already has the
  `SECRET_SLOTS` tuple at line 4.
- `web/src/pages/secrets/resource-block.tsx:47-50` re-implements
  epoch coercion (`Number(provenance.created_at)` → `new Date`)
  that the shared `normalizeEpoch` helper in
  `web/src/lib/epoch.ts:14-21` already provides.

Source review:
`tickets/_review/web_admin_spa_2026-05-23.md` F-009, F-010.

## Scope

### `secrets/model.ts:51-55`

Replace:

```ts
for (const g of groups.values()) {
  g.env.sort((a, b) => a.key.localeCompare(b.key));
  g.files.sort((a, b) => a.key.localeCompare(b.key));
  g.libsecret.sort((a, b) => a.key.localeCompare(b.key));
}
```

with:

```ts
for (const g of groups.values()) {
  for (const slot of SECRET_SLOTS) {
    g[slot].sort((a, b) => a.key.localeCompare(b.key));
  }
}
```

### `secrets/resource-block.tsx:47-50`

Replace:

```ts
const provenanceDate =
  provenance && Number(provenance.created_at)
    ? new Date(Number(provenance.created_at) * 1000)
    : null;
```

with the shared helper:

```ts
const provenanceEpoch = normalizeEpoch(provenance?.created_at);
const provenanceDate =
  provenanceEpoch === null ? null : new Date(provenanceEpoch * 1000);
```

Add `import { normalizeEpoch } from "@/lib/epoch";`.

### Final simplification pass

After both edits, re-read both files against the category 7a
bullet list (review skill, "Simplification") and drop anything
left over: needless intermediates, defensive branches that can't
fire, redundant `Number()` calls, etc.

## Out of scope

- Reformatting the provenance display (still renders the same
  date string).
- Touching other `Number(...)` coercions in unrelated files.

## Acceptance criteria

- [ ] `grep -n "g\.env\.sort\|g\.files\.sort\|g\.libsecret\.sort"
      web/src/pages/secrets/model.ts` returns no hits.
- [ ] `grep -n "Number(provenance" web/src/pages/secrets/resource-block.tsx`
      returns no hits.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes.
