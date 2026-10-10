# RF-0033 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `W-08`.
Original review owners: `web.md`, `report.md`, `finding-index.md`, `input-ledger.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Compare `web/src/pages/stores/table.tsx:10-13` and `web/src/pages/stores/dialogs/delete.tsx:69-75` with `crates/underwrap-backend/src/admin_api/stores/crud.rs:539-566` and `docs/operations.md:199`.

## Recorded observations

The locked branch has no contents-inspection lease and conservatively requires force.
The UI nevertheless describes known contents, while the table's usage text calls them unknown.
This is source evidence; no delete or fresh rendered locked-force-delete check was performed for the finding.

## Reproduction or verification procedure

1. Trace `can_force_delete`/`can_delete_now` into the tooltip and `requiresForce` warning.
2. Compare the backend's locked `None` lease branch with its inspected ready/unlocked branch.
3. For future authorized UI verification, render unknown-locked, known-nonempty, known-empty and mapped cases using decision metadata only.

The first two steps describe source inspection, not an executed deletion.

## Limits and missing evidence

No actual contents, unsafe permission, material removal or live disclosure was observed.
Busy/recovery-required states must not be assumed force-deletable from this locked-state finding.
Separate mapped-store wording about disabling clients was an unpromoted contextual lead because ordinary mapped-row deletion is disabled; it is not an additional demonstrated flow here.
Manual cleanup wording/command findings remain separate.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
