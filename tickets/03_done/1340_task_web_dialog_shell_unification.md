# Task: Web dialog shell unification; EditStore split; RevealSecret shell

Resolves: W-DLG-3, W-DLG-4, W-DLG-5, W-DLG-6, W-DLG-7 (remainder),
W-DLG-8, W-DLG-9.

## Problem

Each SPA dialog repeats Radix's `onOpenChange` ↔ `onClose` translation,
sometimes also rolls its own Dialog (instead of `<ConfirmDialog>`),
and a couple of bigger dialogs mix concerns that should be separate.

## What to do

### 1. Single `<AdminDialog>` shell (W-DLG-8)

Add `web/src/components/admin-dialog.tsx` (or similar) wrapping Radix:

```tsx
<AdminDialog open={open} onClose={onClose} title=... description=...>
  ...
</AdminDialog>
```

…that internally does
`<Dialog open={open} onOpenChange={(o) => !o && onClose()}>`. Convert
every dialog to use the wrapper. The 20+ verbatim translation sites
disappear.

### 2. Split `EditStoreDialog` (W-DLG-3)

`web/src/pages/stores/dialogs/edit.tsx` (197 lines) currently does
two unrelated jobs: edit description AND rotate key (with a checkbox-
gated confirm path and a `<KeyRevealDialog>` follow-up).

- Keep `EditStoreDialog` for description editing only (<100 lines).
- Create `web/src/pages/stores/dialogs/rotate-key.tsx` for the
  rotate-key flow (`<RotateStoreKeyDialog>`). Trigger from the
  table action menu alongside Edit/Delete/Lock.

### 3. Use `dirtyFields` in EditClientInstanceDialog (W-DLG-4)

`web/src/pages/client-instances/dialogs/edit.tsx:77-108` computes
`displayNameChanged`, `storeChanged`, `certChanged`, `autoCreateChanged`
manually. Replace with `form.formState.dirtyFields` and submit only
changed keys. The PUT endpoint is idempotent so this is just an
ergonomic cleanup.

### 4. Refactor DeleteStoreDialog (W-DLG-5)

`web/src/pages/stores/dialogs/delete.tsx` is the only delete dialog
that rolls its own `<Dialog>` rather than using `<ConfirmDialog>`,
and it embeds a post-success second-screen (`cleanupHint`) in the
same component.

- Convert the confirm to `<ConfirmActionDialog>` (or `<ConfirmDialog>`
  + `useConfirmMutation` from 1300).
- Move the cleanup-hint follow-up into a separate
  `<CleanupHintDialog>` opened by the parent on success.
- Pair with `1050_task_structured_store_delete_cleanup_hint` —
  consume the structured `{data_dir, command}` shape in the new
  dialog.

### 5. Centralise `KeyRevealDialog` non-dismissable behaviour (W-DLG-6)

`web/src/pages/stores/dialogs/key-reveal.tsx:50-71` has three
preventDefault handlers (`onEscapeKeyDown`, `onPointerDownOutside`,
`onInteractOutside`) all gated on `!ack`. `onInteractOutside`
subsumes `onPointerDownOutside` in Radix — drop the redundant one.
Consider extracting a `<NonDismissibleDialog ack>` if the pattern
recurs.

### 6. `RevealSecretView` owns its shell (W-DLG-9)

`web/src/pages/secrets/dialogs/reveal.tsx` and
`web/src/pages/secrets/libsecret/dialogs/reveal-item-secret.tsx`
share `RevealSecretView` for the body but each wraps it in their
own `<Dialog>` boilerplate. Have `RevealSecretView` own the shell
and accept `title` + `description` props.

### 7. ConfirmDialog pending state (W-DLG-7)

`web/src/components/confirm-dialog.tsx:23,46-56` — accept
`pending: boolean` from the caller (the `useConfirmMutation` hook
introduced in 1300 returns it). Drop the internal pending state.

## Acceptance

- `cd web && npm test`
- `cd web && npm run test:e2e`
- `cd web && npm run build`
- Manual: every dialog still opens/closes correctly via Escape,
  outside-click, X button, and explicit cancel/confirm.
- Grep: no `onOpenChange={(o) => !o && onClose()}` remaining in
  page files (only inside `AdminDialog`).
