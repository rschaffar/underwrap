# Task: Test the key-reveal / rotate-key / cleanup-hint / lock flows

## Problem

Finding F-14 (must-fix) in
`tickets/_review/stores-client-instances-ui_2026-05-31.md`.

`stores.test.tsx` already covers unlock (`postStoreUnlock`, :158-200), the
locked badge + Unlock button (:135), create-dialog open (:95), and
force-delete confirmation (:112). The genuine gaps are the
security-sensitive **reveal** direction and its neighbours — there is no
test that:

- renders `KeyRevealDialog` with the `key_hex` the server returns from
  create-encrypted or rotate-key,
- verifies it stays non-dismissible until the ack checkbox is ticked and
  that `onClose` is gated on acknowledgement,
- exercises the rotate-key flow, the post-force-delete `CleanupHintDialog`,
  or the Lock (vs Unlock) dialog.

This is the only human-visible master-key surface; a regression here could
leak or hide the key.

## What to do

Add cases (reusing the existing `fetchMock` harness in `stores.test.tsx`):

1. create-encrypted-store → `KeyRevealDialog` renders the returned
   `key_hex`; ack checkbox gates dismissal.
2. rotate-key → same reveal + ack-gate behaviour.
3. force-delete → `CleanupHintDialog` renders.
4. Lock dialog.

## Acceptance

- `cd web && npm test` — do **not** weaken the ack-gate assertions to make
  a test pass.
- `cd web && npm run build`
