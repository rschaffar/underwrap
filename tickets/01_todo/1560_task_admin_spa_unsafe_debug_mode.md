# Task: Admin SPA — unsafe-debug-until mode + confirm dialog

Part of: `epic_single_instance_wrapper_redesign_2026-05`.
Depends on: `1540` (OpenAPI carries the new mode).

## Problem

The wrapper-permission edit cell lets the operator pick `locked` /
`unlocked` / `unlocked-until`
(`web/src/pages/wrapper-permissions/dialogs/edit-cell.tsx`,
~lines 140–164). Add a fourth `unsafe-debug-until` option that
requires an explicit confirmation and renders with a warning style.

## What to do

### 1. Regenerate the client

After ticket 1540, regenerate `web/src/api/schema.ts` from the
backend OpenAPI doc. The TS `WrapperApprovalMode` union should now
include `{ kind: "unsafe-debug-until"; until_epoch: number }`.

### 2. `ConfirmDangerDialog` component

Add `web/src/components/ConfirmDangerDialog.tsx`:

```ts
type Props = {
  open: boolean;
  title: string;
  description: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
};
```

Warning palette (amber/rose), cancel focused by default.

### 3. Wire into `edit-cell.tsx`

- Extend `kind` to include `"unsafe-debug-until"`; add the radio
  (placed last, warning-styled, with a short danger label).
- Default the TTL picker to `now + 15 min` when freshly selected
  (shorter than the 30-min `unlocked-until` default).
- Intercept the radio `onChange`: if the new value is
  `"unsafe-debug-until"` and the previous value was not, open
  `ConfirmDangerDialog` instead of committing; commit on confirm,
  revert on cancel. Adjusting the TTL while already in this mode
  does **not** re-prompt.
- On save, build `{ kind: "unsafe-debug-until", until_epoch }` and
  post via the existing `putWrapperPermission` path.

The danger copy should explain: this unlocks the debug tool
(`uw-<instance> debug <wrapper>`), which prints the wrapper's
projected secret material to the operator's terminal, until the TTL
expires.

### 4. Badge — `table.tsx`

Extend `ModeBadge` (~lines 166–220): `unsafe-debug-until` active →
amber + `AlertTriangle` + `unsafe-debug until <time>`; expired →
rose + `unsafe-debug expired`. Keep it visually distinct from
`unlocked-until`.

### 5. Tests

- Vitest: `ConfirmDangerDialog` (render, focus order, callbacks);
  `edit-cell.tsx` flow (picking the mode from `locked` opens the
  dialog; cancel reverts; confirm commits; re-pick while already in
  the mode does not re-open).
- Playwright: open edit dialog → pick `unsafe-debug-until` → confirm
  → set 15-min TTL → save → assert the warning badge.

## Acceptance

- `cd web && npm test` clean.
- `cd web && npm run test:e2e` clean (inside `nix develop`).
- Manual: the confirm modal appears on first selection, not on TTL
  edits; badge shows the warning style.
