# Epic: Admin SPA shared-infrastructure refinement

## Goal

Tighten the cross-cutting plumbing of the admin SPA for the first release.
Source: `tickets/_review/shared-infra-ui_2026-05-31.md` (20 findings, 2
must-fix).

The shared infra is the root of several cross-report findings:

- The `useFormDialog` / `useConfirmMutation` generic typing (F-20) is the
  cause of the redundant casts flagged in the secrets,
  wrapper-permissions, and stores reviews. Ticket **1430** fixes it and is
  a prerequisite for the cast-removal work in tickets 1100, 1200, and 1310.
- The `ConfirmDangerDialog` rename (F-02/F-03, ticket **1460**) also
  resolves wrapper-permissions finding F-08.

## Scope

Two security-adjacent gaps (auth guard, open-redirect defence), the hook
typing fix, the oversized API client split, the confirm-dialog
naming/contract normalization, dead-code removal, the router functional-API
migration, and stale-comment cleanup.

## Children

- 1410 — auth guard: reject `{ username: null }` (must-fix)
- 1420 — router + confirm-dialog tests (`safeNextPath` must-fix)
- 1430 — hook result typing + invalidation dedupe (**prerequisite for
  1100 / 1200 / 1310**)
- 1440 — dead code & micro-simplification
- 1450 — doc & comment hygiene
- 1460 — rename `ConfirmDangerDialog` + normalize `open` (**resolves
  wrapper-permissions F-08**)
- 1470 — split `api/client.ts` into per-domain endpoint modules
- 1480 — migrate router to TanStack functional API

## Acceptance (epic-level)

All children green on `cd web && npm test` and `cd web && npm run build`;
router/auth changes (1410, 1420, 1480) additionally green on
`cd web && npm run test:e2e`.
