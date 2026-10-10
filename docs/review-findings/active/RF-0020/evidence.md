# RF-0020 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `L-03`.
Original review owners: `store-lifecycle.md`, `substantive-inputs.md`, `coordinator-notes.md`, `documentation-replacement.md`, `report.md`, `input-ledger.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Incorrect text: `web/src/pages/stores/dialogs/cleanup-hint.tsx:29-30`; actual effect: `crates/underwrap-backend/src/admin_api/stores/crud.rs:591-599` and `crates/underwrap-backend/src/store_registry.rs:307-312`.
Owner terminology: `docs/backend.md:24-26,208` and `docs/configuration.md:210-213`.

## Recorded observations

The lifecycle reviewer read the complete consumer and backend deletion flow.
The coordinator independently confirmed the YAML wording versus dynamic JSON registry effect.
The documentation reviewer retained this finding without assigning a duplicate.
No rendered-browser or live deletion experiment was performed for the wording mismatch.

## Reproduction or verification procedure

The recorded method was source inspection, not execution:

1. Read the cleanup-hint sentence and follow the deletion response to registry removal.
2. Compare the registry unlink with the dynamic-registry ownership described in Backend and Configuration; identify that no YAML write occurs.
3. For a future wording regression, render the component with a synthetic cleanup response and assert registry terminology plus retained-data warnings.
   A real destructive cleanup is unnecessary and must not be used as a wording test.

## Limits and missing evidence

No browser interaction, operator mistake or security impact was observed.
This does not claim automatic data deletion or erasure and does not close the adjacent shell-safety finding.
No new source inspection or product execution occurred during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
