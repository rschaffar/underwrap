# RF-0038 — Compound workspace-reservation failure may leave inconsistent recovery state

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

The conditional conjunction is specific: exclusive migration-workspace directory creation succeeds; a subsequent permission or metadata operation fails; and the helper's best-effort directory removal also fails.
`create_private_directory` discards the cleanup error at `crates/underwrap-backend/src/admin_api/stores/lifecycle/workspace.rs:9-17`.
The caller at `crates/underwrap-backend/src/admin_api/stores/lifecycle/migration.rs:184-207` treats the returned reservation error as ordinary `migrate_stage_failed`/`RestoreSource` without establishing that its newly created exact workspace is absent.

A filesystem becoming read-only after mkdir is an illustrative possible fault conjunction, not a reproduced incident.
The triggering directory must have been created by this attempt; a pre-existing collision is a separate no-authority-to-delete case.

## Consequence and boundary

The source-derived candidate is inconsistent live versus restart disposition.
The live entry returns to ordinary source state while an exact migration sibling remains.
Startup intentionally treats any exact-name migration residue as RecoveryRequired, even without a manifest (`crates/underwrap-backend/src/store_registry.rs:209-250`).
Thus an unchanged source could suffer later operational interruption after restart.

At this point the residue is empty: stage creation and secret copying have not occurred.
Neither secret-bearing residue nor corruption, lost keys, data loss or confidentiality impact has been demonstrated.
This must remain needs-verification rather than an accepted failure merely because the individual error branches exist.

The narrow candidate qualifies broad ordinary-error cleanup/recovery language in `docs/security.md:103-105` and `docs/backend.md:265-276`.
It does not invalidate deliberate retained rotation generations, conservative recovery after source-authority failures, inherited backup-mode handling, trusted-host assumptions or the absence of power-loss guarantees.

## Correction and alternatives

First obtain deterministic evidence at the helper's internal boundary.
If the path is confirmed, retain ordinary pre-effect reservation errors when nothing was created, but explicitly distinguish a created-by-this-attempt directory whose removal cannot be confirmed.
Report live recovery/residue status consistently with startup.
Ownership must remain explicit: never clean up a pre-existing collision occupant based only on a matching name.
No automatic general recovery or retained-generation cleanup is proposed.

## Acceptance criteria

- Inject create success, chmod/stat failure and removal failure in one attempt at the actual helper boundary.
  Record return code/disposition, live lifecycle, exact retained pathname and whether stage/material ever existed.
- Reinitialize the registry against the same disposable state and compare startup disposition without claiming a full service reboot if only the initializer ran.
- Include create-failure/no-effect, metadata-failure/successful-cleanup, normal reservation, and pre-existing collision controls.
- Assert source contents/record remain unchanged and collision occupants are preserved.
- Decide confirmation or rejection from this evidence before selecting a fix; existing phase failpoints do not establish it.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
