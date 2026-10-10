# RF-0026 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `T-05`.
Original review owners: `test-harness.md`, `coordinator-notes.md`, `report.md`, `finding-index.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Loops and timeout configuration: `crates/integration-tests/src/harness/readiness.rs:171-223`; reservation ownership: lines 93-169.
`docs/testing.md:248-258` distinguishes awaited shutdown, cancellation-unsafe work, the external suite bound and cooperative port reservation.

## Recorded observations

The specialist and coordinator independently inspected the 3,000 iterations, 500-ms request timeout and 20-ms sleep.
The 1,560-second estimate is derived arithmetic, not an observed duration or hard upper bound including scheduling.
No 26-minute stall was deliberately run.

The Rust sibling test at `crates/integration-tests/src/harness/readiness/tests.rs:1-69` covers lock exclusion, expected lock error, CLOEXEC and release, not stalled-request elapsed time.
Browser support instead has an absolute deadline and an abortable request at `web/e2e/backend.ts:384-437`; its simulated stalled-probe check is not evidence for these Rust loops.

## Reproduction or verification procedure

1. Read both loop bodies and include awaited request duration, not only sleep duration.
2. Compute the all-timeouts case separately from immediate errors and early success.
3. Trace the admin reservation's lifetime through readiness.
4. Future authorized execution should use a harmless local stalled-response fixture and a prompt-response control under an external process deadline; report actual elapsed measurements and cleanup separately.

Steps 1-3 are source inspection.
Step 4 was not performed.

## Limits and missing evidence

No observed hang, production denial of service or universal fixture cleanup guarantee is claimed.
Prior passing Rust and Node suites do not fill the missing Rust stalled-probe witness.
An in-runtime timeout around cancellation-unsafe joins would not meet the maintained ownership contract.
No new server, test or dependency was created during this migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
