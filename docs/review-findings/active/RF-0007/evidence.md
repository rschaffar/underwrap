# RF-0007 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `S-03`.
Original review owners: `secret-service.md`, `substantive-inputs.md`, `coordinator-notes.md`, `documentation-replacement.md`, `report.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Missing object-server removal: `crates/underwrap-secretsd/src/dbus_service/item.rs:40-54` and `crates/underwrap-secretsd/src/dbus_service/collection.rs:70-81`.
Ignored setter results: `crates/underwrap-secretsd/src/dbus_service/item.rs:95-102,139-144,166-171` and `crates/underwrap-secretsd/src/dbus_service/collection.rs:195-201`.
Missing-item model behavior: `crates/underwrap-secretsd-store/src/store/items.rs:28-45`.

## Recorded observations

The specialist and coordinator independently traced the exported lifetime, false result and successful publication-return path.
Model tests at `crates/underwrap-secretsd-store/src/store/tests.rs:396-451` assert false/None and exact unchanged state on missing objects.
Collection deletion tests at `crates/underwrap-secretsd/src/dbus_service/collection/tests.rs:89-128` inspect the store rather than post-delete registration.
Existing typed item tests cover ACK/reject on extant items, not missing-item setter outcomes.

No fresh post-delete wire reproduction, registry-growth measurement, elapsed time or command exit is recorded for this finding.
The source-derived obsolete-interface accumulation consequence was explicitly preserved by independent synthesis.

## Reproduction or verification procedure

Recorded method: source inspection, not execution.
Future normalized procedure:

1. Prepare a disposable private Secret Service/bus with valid synthetic collection/item IDs, empty/nonsecret material and an identified publication ACK receiver.
   Obtain both object paths and retained interface references.
2. Accept item deletion, then try every item setter using those references.
   Compare response outcome, accepted state and subsequent GetSecret; do not print secret bodies.
3. Repeat after collection deletion for descendant item setters and collection Label.
   Inspect actual exported dispatch as well as model state.
4. Reject a delete and verify state/interface retention; include accepted/rejected setters on an existing item as controls.
5. Repeat bounded create/delete cycles and inspect obsolete object registration directly.
   Reap disposable processes.
   If using a simulated ACK peer, explicitly separate that from real namespace/persistence evidence.

## Limits and missing evidence

The concrete chain is source-confirmed but not freshly wire-executed.
No universal D-Bus/session conformance, cross-resource disclosure, whole-process memory bound or denial-of-service assurance is inferred.
The host ACK is not proof of backend persistence or caller receipt.
No new source review, live-bus experiment or implementation occurred during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
