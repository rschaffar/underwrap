# RF-0018 — Permission-cleanup diagnostics mislabel unavailable store states

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

Remap or delete a client whose former store refuses a storage lease as Busy or RecoveryRequired.
In `crates/underwrap-backend/src/admin_api/policy.rs:63-84`, `drop_client_instance_permissions` groups those reasons with Locked, logs store-locked/unlock guidance and emits operation `store-locked`.

This helper is best-effort cleanup after the client authority change has committed.
A failed lease therefore leaves old permission rows but does not undo the remap/deletion.
The comment at `crates/underwrap-backend/src/admin_api/policy.rs:54-55` additionally describes self-healing on unlock, although the retained-row cleanup is manual.

## Consequence and boundary

An operator receives the wrong explanation and recovery action.
Ordinary unlock cannot resolve RecoveryRequired, and Busy is not the same lifecycle condition as an encrypted locked store.
The diagnostic can delay correct reconciliation or imply automatic cleanup that never occurs.

This is Low observability/operator-guidance debt, not a permission bypass.
Committed new authority remains committed; denial while the old store is unavailable remains intact.
Retained stale rows themselves are documented best-effort behavior, not the defect.
`docs/backend.md:183-193,213-233` distinguishes those authority, cleanup and lifecycle responsibilities.

## Correction and alternatives

Keep postcommit cleanup best effort, but preserve the actual lease-failure reason in stable, value-free diagnostics and give applicable follow-up.
Distinguish unlocking a Locked store, later retry/manual reconciliation for Busy, and the documented recovery procedure for RecoveryRequired.
Remove the automatic-self-healing implication unless a separately designed mechanism is actually added.
This record does not request that mechanism, atomic multi-registry changes, migration concurrency or automatic stale-row repair.

## Acceptance criteria

- Cover Locked, Busy and RecoveryRequired cleanup attempts separately, asserting the lifecycle reason and applicable guidance rather than one shared store-locked message.
- Assert successful committed remap/deletion and current new authority despite cleanup failure.
- Assert exact old-policy preservation when the lease is refused, no unauthorized old-store access and no false cleanup-success claim.
- Retain a successful-cleanup control and verify documentation/comments promise only implemented behavior.
  These are future acceptance checks, not results against a fix.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
