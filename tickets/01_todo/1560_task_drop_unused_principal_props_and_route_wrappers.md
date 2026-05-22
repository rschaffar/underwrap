# Task: Drop unused `principal` prop and collapse route wrappers

Parent: `1500_story_web_spa_refinement.md`
Resolves: F-010, F-024, F-033 (review report
  `web_spa_2026-05-22.md`)

## Background

### F-010 — `principal` prop is dead on four pages

Four pages declare a `principal?: WhoamiResponse | null` prop and
never read it (the parameter is prefixed `_` in three of them,
and one has a multi-line comment apologising for keeping it):

- `web/src/pages/stores.tsx:41-45, 87` — `StoresPageProps`,
  `_props`. Comment: "Currently unused — all authenticated users
  have full access — but retained so the route call sites don't
  need to change."
- `web/src/pages/wrapper-permissions.tsx:49-51, 176` —
  `WrapperPermissionsPageProps`, `_props`.
- `web/src/pages/secrets.tsx:53-60, 120` — `SecretsPageProps`,
  `_principal`.
- `web/src/pages/client-instances.tsx:54-56` — inline
  `principal?: WhoamiResponse | null` parameter.

`UsersPage` and `ProfilePage` legitimately consume `principal`
(meUsername / "(you)" marker, profile header). Leave those.

### F-024 — seven route wrappers exist only to forward `principal`

`web/src/router.tsx:171-225` defines six per-page wrapper
components (`StoresRouteComponent`, `ClientInstancesRouteComponent`,
`UsersRouteComponent`, `ProfileRouteComponent`,
`WrapperPermissionsRouteComponent`, `SecretsRouteComponent`) that
each call `route.useRouteContext()` and render the page with
`principal={ctx.principal}`. Once F-010 removes the prop on the
four pages above, four of the wrappers collapse.

### F-033 — long comments justifying the dead prop

The comments above the prop declarations document why the prop is
retained. They go away with the prop.

## Scope

- Drop the `principal?:` field from
  `StoresPageProps` / `WrapperPermissionsPageProps` /
  `SecretsPageProps`, and remove the inline `principal?:`
  parameter from `ClientInstancesPage`.
- Remove the page-parameter destructuring (`{ principal: _principal }`,
  `_props`) on those four pages.
- Delete the multi-line comments that referenced the prop.
- In `web/src/router.tsx:171-225`:
  - `storesRoute`: drop `StoresRouteComponent`; set
    `component: StoresPage`.
  - `clientInstancesRoute`: drop `ClientInstancesRouteComponent`;
    set `component: ClientInstancesPage`.
  - `wrapperPermissionsRoute`: drop
    `WrapperPermissionsRouteComponent`;
    set `component: WrapperPermissionsPage`.
  - `secretsRoute`: drop `SecretsRouteComponent`; set
    `component: SecretsPage`.
  - `usersRoute` and `profileRoute` keep their wrappers — those
    pages still consume `principal`.
- Update unit tests that constructed pages with
  `principal={{ authenticated: true }}` etc. — drop the prop from
  the JSX. Affected files:
  - `web/src/pages/stores.test.tsx:88-95, 105-115, 127-136,
    149-159, 184-194, 219-230`
  - `web/src/pages/secrets.test.tsx:76, 116-123, 169`
  - `web/src/pages/client-instances.test.tsx:159, 170-172,
    186-188, 202-204, 219-221, 243-245, 261-263, 280-282`
  - `web/src/pages/wrapper-permissions.test.tsx` — passes no
    props today, no test change.
- `router.test.tsx` already inspects only path strings; nothing
  to change.

## Acceptance criteria

- [ ] `rg -n 'principal\?' web/src/pages/stores.tsx
      web/src/pages/wrapper-permissions.tsx
      web/src/pages/secrets.tsx
      web/src/pages/client-instances.tsx`
      returns nothing.
- [ ] `rg -n 'StoresPageProps|SecretsPageProps|WrapperPermissionsPageProps' web/src/`
      returns nothing (the prop types are gone or renamed).
- [ ] `rg -n 'RouteComponent' web/src/router.tsx` returns at most
      two hits (`UsersRouteComponent`, `ProfileRouteComponent`).
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes.
