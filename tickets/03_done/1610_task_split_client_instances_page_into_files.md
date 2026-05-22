# Task: Split `pages/client-instances.tsx` into per-dialog files

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-004 (review report `web_spa_2026-05-22.md`). Partially
  touches F-023 — the `toDetail` shim — see Scope.

## Background

`web/src/pages/client-instances.tsx` is 715 LoC and hosts:

- `ClientInstancesPage`
- `formatEpoch` (moved to `lib/epoch.ts` in 1510),
  `toDetail` (the `*ListItem` → `*DetailResponse` shim),
  `statusMatches`, `ClientStatusFilter` type
- `StatusBadge`, `AutoCreateBadge`
- Dialogs: `CreateClientInstanceDialog`,
  `EditClientInstanceDialog`, `DeleteClientInstanceDialog`,
  `SetStatusDialog`

## Scope

Restructure as:

```
web/src/pages/client-instances/
  page.tsx              // ClientInstancesPage + ClientStatusFilter
                        // + statusMatches
  badges.tsx            // StatusBadge + AutoCreateBadge
  dialogs/
    create.tsx          // CreateClientInstanceDialog
    edit.tsx            // EditClientInstanceDialog
    set-status.tsx      // SetStatusDialog
    delete.tsx          // DeleteClientInstanceDialog
```

- Re-point the router import in `web/src/router.tsx:20` from
  `@/pages/client-instances` to `@/pages/client-instances/page`.
- Re-point `web/src/pages/client-instances.test.tsx:6` from
  `./client-instances` to `./client-instances/page`.
- **Decision needed inside this task** on F-023 (`toDetail`
  shim): either keep it as-is in `page.tsx`, or narrow each
  dialog's prop type to a hand-written `ClientInstanceSummary`
  containing only the fields the dialog reads. Recommended:
  narrow the types in this task — it's already touching every
  dialog file; the shim disappears with two-or-three field
  trims per dialog. If the narrowing turns noisy, leave a
  follow-up note in the ticket close commit and route to a
  fresh ticket.

## Acceptance criteria

- [ ] `web/src/pages/client-instances.tsx` is gone; the
      directory layout above exists.
- [ ] `page.tsx` does not exceed 300 LoC.
- [ ] No dialog file exceeds 250 LoC.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes (no e2e spec
      explicitly for client-instances today, but
      `web/e2e/secrets.spec.ts` and others exercise the
      auto-create-on-startup flow that registers an instance —
      verify no regression).
