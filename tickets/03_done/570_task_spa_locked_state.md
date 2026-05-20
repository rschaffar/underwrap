# Task: Admin SPA surfaces lock state with Unlock / Lock actions

Parent: `500_story_in_memory_master_keys.md`
Epic: `epic_in_memory_master_keys.md`

## Scope

- `web/src/pages/stores.tsx`:
  - New "Status" column showing a `Locked` badge for encrypted
    stores that are not yet unlocked, otherwise blank (plain stores
    are always blank).
  - Row actions, gated by `canMutateStores()` admin check:
    - **Unlock** (encrypted + locked): opens a dialog with one
      monospace input "Master key (64 hex characters)". Validate
      `^[0-9a-fA-F]{64}$` client-side before submit. Submit calls
      the new mutation. On error: display the structured `code`
      (`invalid_key_hex`, `key_mismatch`, `already_unlocked`) as a
      friendly message.
    - **Lock** (encrypted + unlocked): confirm dialog ("Lock
      `<name>`? Wrappers will get `store_locked` errors until you
      unlock it again.") and a mutation call.
    - **Rotate key** (encrypted + unlocked, admin only): existing
      dialog plus a new prominent warning "This will decrypt and
      re-encrypt every secret in this store under a new key. The
      store will not accept writes during rotation." Disable the
      button when the store is locked, with tooltip "Unlock the
      store first".
- `web/src/api/client.ts`:
  - `postStoreUnlock(storeId, keyHex)` → POST `/v1/admin/stores/{id}/unlock`.
  - `postStoreLock(storeId)` → POST `/v1/admin/stores/{id}/lock`.
  - Existing `postStoreRotateKey` unchanged in signature.
- `web/src/api/types/`: regenerate via `ts-rs`. New / updated:
  - `StoreSummary.locked: boolean`.
  - `StoreDetail.locked: boolean`.
  - `AdminStoreUnlockRequest { key_hex: string }`.
- Make sure the unlock dialog's `key_hex` value never lands in
  `localStorage` / `sessionStorage` / URL query / React Query cache.
  Use uncontrolled input or local component state only; clear it
  in the form's `onSubmit` finalizer regardless of outcome.
- Tests:
  - Vitest: render the stores page with a locked encrypted store,
    confirm the badge + Unlock action, simulate a successful
    unlock, confirm the row refreshes.
  - Vitest: invalid hex → submit disabled.
  - Playwright: full flow — create encrypted store, see hex once,
    simulate backend restart (or directly call the lock endpoint),
    open Unlock dialog, paste hex, verify the row flips to unlocked.
    Inspect `localStorage` / `sessionStorage` at the end and confirm
    no entry contains the hex.

## Acceptance criteria

- [ ] Locked encrypted stores are visually distinct and offer an
      Unlock action; unlocked ones offer Lock.
- [ ] Rotate is disabled when locked.
- [ ] Key hex is never persisted in browser storage.
- [ ] `cd web && npm test` green; Playwright e2e green inside
      `nix develop`.

## Out of scope

- Bulk unlock-many. Per-store only.
- Remembering "I just created this store, here is the key" past the
  initial one-shot reveal modal.
