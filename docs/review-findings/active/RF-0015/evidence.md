# RF-0015 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `T-02`.
Original review owners: `test-harness.md`, `coordinator-notes.md`, `report.md`, `input-ledger.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The harness reviewer read `crates/integration-tests/src/harness/builders.rs:1-58`, its callers and the startup-reservation flow.
The coordinator independently confirmed the shared libc result access.
The production reentrant-NSS documentation is a contrast, not evidence of execution of this helper.

## Recorded observations

The sequence is `getegid` → `getgrgid` → null check → borrowed `gr_name` C string → owned String.
Shared-result validity must span the last two operations, while a concurrent non-reentrant lookup can change that storage.
`crates/integration-tests/src/harness/builders/tests.rs:1-34` covers projection construction, not concurrent group lookup.

No race reproducer or sanitizer was run for this finding.
The coordinator's normal workspace run used four test threads and passed 942 top-level tests; a passing schedule does not contradict the source-level race or establish pointer lifetime safety.

## Reproduction or verification procedure

1. Inspect the helper's unsafe calls and determine which API owns the record/name storage during copying.
2. Trace callers and the lifetime of the backend startup reservation; distinguish per-test current-thread execution from cross-test Cargo threads.
   These are source-inspection steps, not behavioral execution.
3. After an authorized correction, test concurrent repeated lookups and affected fixture setup with known effective GID, plus ordinary and injected lookup-error controls.
   Report only result/identity metadata needed for the test.
4. Review the ownership proof independently of stress/sanitizer results and run affected harness checks with the documented packaged-artifact prerequisites.

## Limits and missing evidence

No concrete race schedule, corrupt string or wrong-group daemon startup was observed.
Platform/NSS implementation behavior beyond the API lifetime issue was not exhaustively tested.
This is a harness defect, not a production memory-corruption or authorization finding.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
