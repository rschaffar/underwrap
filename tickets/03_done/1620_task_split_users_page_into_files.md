# Task: Split `pages/users.tsx` into per-dialog files

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-005 (review report `web_spa_2026-05-22.md`)

## Background

`web/src/pages/users.tsx` is 629 LoC — marginally over the
~600 LoC guidance. Kept as its own ticket so all six list pages
end up with the same shape rather than leaving one as the
odd-one-out.

Hosts:

- `UsersPage`
- `formatEpoch` (already in `lib/epoch.ts` after 1510)
- `UsersTable`
- Dialogs: `CreateUserDialog`, `EditUserDialog`,
  `ResetPasswordDialog`, `DeleteUserDialog`
- Zod schemas inline next to their dialogs

`UsersPage` legitimately consumes the `principal` prop
(meUsername / "(you)" marker), so unlike the other splits this
page's prop stays.

## Scope

Restructure as:

```
web/src/pages/users/
  page.tsx              // UsersPage (keeps principal prop)
  table.tsx             // UsersTable + UsersTableProps
  dialogs/
    create.tsx          // CreateUserDialog + createSchema
    edit.tsx            // EditUserDialog + editSchema
    reset-password.tsx  // ResetPasswordDialog + resetSchema
    delete.tsx          // DeleteUserDialog
```

- Re-point the router import in `web/src/router.tsx:21` from
  `@/pages/users` to `@/pages/users/page`.
- Re-point `web/src/pages/users.test.tsx:6` from `./users` to
  `./users/page`.
- The route wrapper `UsersRouteComponent` in `router.tsx`
  stays — it forwards `principal`.

## Acceptance criteria

- [ ] `web/src/pages/users.tsx` is gone; the directory layout
      above exists.
- [ ] `page.tsx` does not exceed 250 LoC.
- [ ] No dialog file exceeds 200 LoC.
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes (`web/e2e/users.spec.ts`).
