# RF-0018 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `A-02`.
Original review owners: `backend-data.md`, `substantive-inputs.md`, `coordinator-notes.md`, `report.md`, `input-ledger.md`, `synthesis-verification.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The grouped branch and comment are at `crates/underwrap-backend/src/admin_api/policy.rs:54-84`.
The maintained distinction between committed client mapping, best-effort cleanup and lifecycle actions is at `docs/backend.md:183-193,213-233`.

## Recorded observations

The backend-data reviewer traced the grouped Locked/Busy/RecoveryRequired arm.
The coordinator separately confirmed that all three receive the locked/unlock diagnostic.
Existing `crates/underwrap-backend/src/admin_api/policy/tests.rs:27-75` asserts the Locked case only.
The reviewed committed-remap/failed-cleanup scenario at `crates/underwrap-backend/src/admin_api/client_instances/tests.rs:285-379` supplies adjacent authority/cleanup evidence, not a fresh execution of all three diagnostic branches.

No dedicated Busy or RecoveryRequired runtime reproduction, captured diagnostic output or fix verification was performed.
No execution duration or exit code is attributed to source inspection.

## Reproduction or verification procedure

The recorded method was source inspection, not execution.
A future bounded verification can be reconstructed without any old probe:

1. Trace client remap/delete to the postcommit `drop_client_instance_permissions` call; separate registry publication from subsequent permission cleanup.
2. Follow each unavailable lease result through the match arm and inspect its message, operation/reason and return behavior.
   Compare each with the owner-defined allowed actions.
3. For a regression, use disposable stores and one client with known old permission rows.
   Independently arrange Locked, Busy and RecoveryRequired lease denial, then remap/delete through the real handler.
   Capture only structured reason/action metadata.
4. Check new authority, exact retained old policy and absence of a cleanup-success event.
   Include available-store cleanup as a control.

## Limits and missing evidence

This concerns misleading reporting, not the intentional nontransactional cleanup boundary.
Existing suite success cannot substitute for the missing state-specific diagnostic assertions.
No new repository source inspection, service call, fault injection or test execution occurred during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
