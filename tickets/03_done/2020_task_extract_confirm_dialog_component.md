# Task: Extract a shared `<ConfirmDialog>` component for delete/confirm modals

## Background

Seven dialogs are confirmation modals with the same structure:
title, description, optional warning, error alert, cancel button,
destructive confirm button with pending-state copy. Six differ
only in copy + the mutation function; two add a "blockers" branch
on top.

Source review:
`tickets/_review/web_admin_spa_2026-05-23.md` F-008.

Affected files:

- `web/src/pages/users/dialogs/delete.tsx`
- `web/src/pages/client-instances/dialogs/delete.tsx`
- `web/src/pages/client-instances/dialogs/set-status.tsx`
- `web/src/pages/stores/dialogs/lock.tsx`
- `web/src/pages/secrets/dialogs/delete-secret.tsx`
- `web/src/pages/secrets/dialogs/delete-resource.tsx` (with blockers)
- `web/src/pages/wrapper-permissions/dialogs/delete-pair.tsx` (with blockers)

## Scope

### Add `web/src/components/confirm-dialog.tsx`

Signature:

```ts
interface ConfirmDialogProps {
  title: string;
  description: React.ReactNode;
  /** Extra content rendered between the description and the action row
   *  (warnings, blockers list, etc.). */
  body?: React.ReactNode;
  /** Destructive styles the confirm button as `variant="destructive"`. */
  destructive?: boolean;
  confirmLabel: string;
  pendingLabel: string;
  onConfirm: () => void | Promise<void>;
  onClose: () => void;
  /** When provided, the confirm button is disabled and the dialog
   *  surfaces this as an `<Alert variant="warning">` body above the
   *  buttons (used by delete-with-blockers). */
  warning?: React.ReactNode;
  pending?: boolean;
}
```

Internals:

- Wraps the shared `Dialog`/`DialogContent`/`DialogHeader`/...
  primitives.
- Renders the optional `body` between the description and the
  footer.
- Cancel button is `variant="ghost"`; confirm button is
  `variant="destructive"` when `destructive`, otherwise default.
- The confirm button toggles between `confirmLabel` and
  `pendingLabel` based on `pending`.
- Owns its own `submitError` `useState` and renders it via
  `<Alert>` directly above the footer; expose a way to surface
  errors from the caller's mutation. Simplest: caller's
  `onConfirm` is awaited inside the dialog; if it throws, the
  dialog renders `describeApiError(err)`.

### Convert each call site

1. **delete-secret** (`secrets/dialogs/delete-secret.tsx`) — trivial
   wrap; pass `destructive`, `confirmLabel="Delete"`,
   `pendingLabel="Deleting…"`, `onConfirm={() => mut.mutateAsync()}`.

2. **delete-user** (`users/dialogs/delete.tsx`) — pass `body` =
   `isLastUser ? <Alert variant="warning">…</Alert> : null`. The
   "cannot delete last user" gate stays on the caller side.

3. **delete-client-instance**
   (`client-instances/dialogs/delete.tsx`) — direct wrap.

4. **set-status** (`client-instances/dialogs/set-status.tsx`) —
   non-destructive when re-enabling. Pass `destructive={nextStatus === "disabled"}`.

5. **lock-store** (`stores/dialogs/lock.tsx`) — direct wrap.

6. **delete-resource** (`secrets/dialogs/delete-resource.tsx`) — pass
   `body=<BlockersAlert blockers={mut.data?.blocked_by} />`. The
   blockers JSX moves into a tiny `BlockersAlert` helper local to
   the file (or into `components/blockers-alert.tsx` if you prefer;
   judgement call, but the two blockers dialogs render different
   bodies so a local helper is simpler).

7. **delete-pair** (`wrapper-permissions/dialogs/delete-pair.tsx`) —
   same pattern; local `BlockersAlert` body.

After conversion each dialog should be roughly 15–30 lines down
from 70–140.

## Out of scope

- The two write/edit dialogs (`secrets/dialogs/write.tsx`,
  `users/dialogs/reset-password.tsx`, etc.) — they have form
  state inside, not a fit for `ConfirmDialog`.
- The key-reveal dialog — gated on a separate "I have saved this
  key" ack, different UX.
- The policy viewer — read-only viewer, not a confirm flow.

## Acceptance criteria

- [ ] `web/src/components/confirm-dialog.tsx` exists.
- [ ] Each of the seven dialogs above imports `ConfirmDialog`.
- [ ] `grep -n "DialogContent" web/src/pages/users/dialogs/delete.tsx
      web/src/pages/client-instances/dialogs/delete.tsx
      web/src/pages/client-instances/dialogs/set-status.tsx
      web/src/pages/stores/dialogs/lock.tsx
      web/src/pages/secrets/dialogs/delete-secret.tsx`
      returns zero hits (all consume the wrapper).
- [ ] The two blockers dialogs still render the same blockers UX
      as before (Playwright tests for `secrets.spec.ts` and
      `wrappers.spec.ts` confirm).
- [ ] `cd web && npm test` passes.
- [ ] `cd web && npm run build` passes.
- [ ] `cd web && npm run test:e2e` passes.
