# RF-0025 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `T-04`.
Original review owners: `test-harness.md`, `coordinator-notes.md`, `report.md`, `finding-index.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Primary helper ranges are `crates/integration-tests/src/harness/mod.rs:475-477,955-983,1035-1041`.
Masking preparation is `crates/integration-tests/src/tests/client_instances.rs:194-205,227-233`.

## Recorded observations

The hardcoded default store is used only for seeding; the later API calls still address the caller-supplied store.
The original specialist fully read harness support and sibling harness tests and found no fresh, initially unseeded non-default-store witness.
The coordinator independently inspected the seeding mismatch.
Neither recorded a focused two-store reproduction.

The coordinator's broader workspace suite passed 942 top-level tests, including 114 integration tests.
That result does not contradict a helper defect hidden by pre-seeding; it is not a new execution of the missing setup case.

## Reproduction or verification procedure

Source inspection, not executed setup:

1. Follow the store argument through both public permission helpers.
2. Follow `ensure_pair` into the hardcoded default-store delegation.
3. Compare the target of subsequent permission list/delete/upsert requests.
4. Read the remap test's preparation to identify why both references are already available.
5. Future authorized verification should begin without either pair and compare both stores' decision/state metadata through success and failure.

## Limits and missing evidence

No observed production cross-store mutation, authorization bypass, live fixture corruption or data-loss incident is claimed.
The possible earlier target-row deletions follow the inspected helper sequence; they were not forced in execution.
Existing remap assertions are not discarded, and no backend reference check or test source was changed during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
