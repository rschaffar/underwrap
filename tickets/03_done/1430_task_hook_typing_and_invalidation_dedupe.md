# Task: Type hook results, dedupe invalidation, drop dead hook surface

## Problem

Findings F-20, F-10, F-08, F-07 in
`tickets/_review/shared-infra-ui_2026-05-31.md`:

- `use-form-dialog.ts:24,32` — `submit` returns `Promise<unknown>` and
  `onSuccess` receives `result: unknown`, forcing call-site casts in the
  page dialogs (`result as AdminStoreCreateResponse`,
  `(await confirm()) as { key_hex; warning }`). The `submit`-argument casts
  in the pages are already redundant under the `TRequest = TValues`
  default — the real gap is the `unknown` result.
- `use-form-dialog.ts:63-68` and `use-confirm-mutation.ts:38-42` — an
  identical query-invalidation block.
- `use-confirm-mutation.ts:21,54` — `clearError` is exported but never
  called (`confirm()` already clears at its top).
- `use-client-instance-directory.ts:24,28` — the `useMemo` re-reads
  `query.data?.items` instead of the derived `clientInstances`, and its
  dep array is only `[query.data]`.

## What to do

1. Add a `TResult` generic to `useFormDialog` and `useConfirmMutation` so
   `submit`/`confirm` is `() => Promise<TResult>` and `onSuccess` receives
   `result: TResult`. This removes the result casts in the page dialogs.
2. Extract `invalidateAll(qc, keys)` into `web/src/lib/query.ts`; call it
   from both hooks.
3. Remove `clearError` from the `useConfirmMutation` interface and return.
4. Fix `useClientInstanceDirectory` to iterate the derived
   `clientInstances` and align the dep array.

## Acceptance

- `cd web && npm test`
- `cd web && npm run build`

## Note

This is a **prerequisite** for the cast-removal work in secrets ticket
1100, wrapper-permissions ticket 1200, and stores ticket 1310. Land it
first.
