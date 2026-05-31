# Story: Shared-infra refinement work items

Parent: `epic_admin_shared_infra_refinement.md`
Source report: `tickets/_review/shared-infra-ui_2026-05-31.md`

## Summary

Carries the eight shared-infra tasks (1410–1480) and their cross-report
sequencing.

## Sequencing notes

- **1430 (hook result typing) must land before** the cast-removal portions
  of secrets ticket 1100, wrapper-permissions ticket 1200, and stores
  ticket 1310. Those tickets reference 1430 explicitly.
- **1460 (ConfirmDangerDialog rename)** resolves wrapper-permissions
  finding F-08; no separate ticket exists in the wrapper-permissions plan.
- The two must-fix tasks (1410 auth guard, 1420 `safeNextPath` test) are
  independent and can land first.
- 1470 (client split) is the largest mechanical move; sequence it so it
  does not collide with other in-flight edits to `api/client.ts`.

## Acceptance

Each child ticket carries its own acceptance check.
