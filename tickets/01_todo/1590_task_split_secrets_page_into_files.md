# Task: Split `pages/secrets.tsx` into per-dialog files

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-001 (review report `web_spa_2026-05-22.md`)

## Background

`web/src/pages/secrets.tsx` is 1385 LoC — the largest file in the
SPA. It hosts:

- `SecretsPage`
- Model + helpers: `SECRET_SLOTS`, `SecretSlot`, `SecretEntry`,
  `SecretAddress`, `SecretResourceGroup`, `resourceCount`,
  `groupSecretEntries`
- View components: `ResourceBlock`, `SlotSection`,
  `EntryActions`
- Dialogs: `RevealSecretDialog`, `WriteSecretDialog`
  (create + edit), `CreateResourceDialog`,
  `DeleteSecretDialog`, `RenameResourceDialog`,
  `DeleteResourceDialog`
- Zod schemas inline next to their dialogs

`groupSecretEntries`, `SECRET_SLOTS`, `resourceCount`, the
address/group types are imported by `secrets.test.tsx:5` —
those need to stay reachable.

## Scope

Restructure as:

```
web/src/pages/secrets/
  page.tsx              // SecretsPage
  model.ts              // SECRET_SLOTS, SecretSlot, SecretEntry,
                        // SecretAddress, SecretResourceGroup,
                        // resourceCount, groupSecretEntries
  resource-block.tsx    // ResourceBlock + SlotSection + EntryActions
  dialogs/
    reveal.tsx          // RevealSecretDialog
    write.tsx           // WriteSecretDialog (create + edit)
                        // + writeSchema
    create-resource.tsx // CreateResourceDialog + resourceSchema
    delete-secret.tsx   // DeleteSecretDialog
    rename-resource.tsx // RenameResourceDialog
                        // + renameResourceSchema
    delete-resource.tsx // DeleteResourceDialog
```

- Re-point the router import in `web/src/router.tsx:18` from
  `@/pages/secrets` to `@/pages/secrets/page`.
- Re-point `web/src/pages/secrets.test.tsx:5` imports:
  - `SecretsPage` from `./secrets/page`.
  - `groupSecretEntries` from `./secrets/model`.
- `describeCreator` and the resource/client-instance lookup
  Maps have already been extracted to `lib/creator.ts` and
  `hooks/use-store-lookups.ts` in tickets 1510 / 1520; the
  new page file consumes them via those imports rather than
  carrying inline copies.

## Acceptance criteria

- [ ] `web/src/pages/secrets.tsx` is gone; the directory layout
      above exists.
- [ ] `page.tsx` does not exceed 300 LoC.
- [ ] No dialog file exceeds 250 LoC.
- [ ] `cd web && npm test` passes (secrets.test re-imports from
      the new locations).
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes (`web/e2e/secrets.spec.ts`).
