# RF-0006 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `S-01`.
Original review owners: `secret-service.md`, `substantive-inputs.md`, `coordinator-notes.md`, `documentation-replacement.md`, `report.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Startup copy: `crates/underwrap-secretsd/src/main.rs:220-230`; map-only SetAlias: `crates/underwrap-secretsd/src/dbus_service/service.rs:198-232`; CreateCollection registration: `crates/underwrap-secretsd/src/dbus_service/service.rs:86-97`; fixed-ID operations: `crates/underwrap-secretsd/src/dbus_service/collection.rs:52-63,70-81,105-144`.

## Recorded observations

The Secret Service reviewer followed the complete implementation, and the coordinator separately confirmed that accepted SetAlias does not update/remove/register alias interfaces.
Independent synthesis retained this as source-confirmed, not wire-executed.

Inspected `crates/underwrap-secretsd/src/dbus_service/service/tests.rs:85-110,192-232` checks accepted store/ReadAlias/publication state only.
Registration tests at `crates/underwrap-secretsd/src/dbus_service/service/tests.rs:235-308` inspect collection paths, not alias paths.
Those assertions do not discriminate stale exported routing.
The shared mutation path at `crates/underwrap-secretsd/src/dbus_service/mutation.rs:10-20` still provides candidate publication and rollback.

## Reproduction or verification procedure

The recorded evidence is source inspection, not execution.
A future normalized wire verification requires no historical probe:

1. Start a disposable private bus and current secretsd with two valid synthetic collections and one valid startup alias.
   Use empty/nonsecret values and a real host publication receiver or a clearly identified matching-ACK test peer.
2. Establish baseline ReadAlias and direct alias-path collection operations.
   Retarget via SetAlias, wait for accepted publication, then compare ReadAlias with reads and a harmless mutation made through the alias path.
   Inspect which collection changed without logging material.
3. Repeat for clear, a newly added SetAlias alias, CreateCollection-with-alias and collection deletion.
   Inspect exported dispatch, not just the in-memory alias map.
4. Reject a publication attempt and verify both accepted state and callable routing remain unchanged.
   Stop/reap disposable bus/helper processes.
   A simulated ACK peer is not evidence of full namespace readiness or persistence.

## Limits and missing evidence

No fresh private-bus wire call, dispatch/registration regression or end-to-end persistence case was executed for this finding.
Existing suite passes do not close those missing assertions.
No cross-resource leak, malformed-publication acceptance, per-client isolation failure or complete D-Bus conformance conclusion follows.
No new source review or execution occurred during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
