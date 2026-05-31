# Task: Migrate router to TanStack `createRootRoute`/`createRoute`

## Problem

Finding F-15 in `tickets/_review/shared-infra-ui_2026-05-31.md`.

`router.tsx` uses the pre-v1 `new RootRoute` / `new Route` constructor API
(lines 49, 97, 135, 143, 149, 155, 164, 173, 179, 191, 214, 230, 285) on
`@tanstack/react-router@^1.169.2`. The documented v1 API is the functional
`createRootRoute` / `createRoute`. A first release is the moment to align.

## What to do

1. Mechanically rename the constructor calls to the functional API. The
   functional signatures take the same options object; route-tree assembly
   (`addChildren`) and type registration are unchanged.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`
- `cd web && npm run test:e2e` (navigation unchanged)
