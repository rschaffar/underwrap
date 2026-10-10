# RF-0029 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `T-08`.
Original review owners: `test-harness.md`, `coordinator-notes.md`, `report.md`, `finding-index.md`, `claim-evidence-join.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Selected comparisons: `crates/integration-tests/src/tests/store_read.rs:299-307,346-354,369-377,433`; further examples: `crates/integration-tests/src/tests/daemon.rs:885` and `crates/integration-tests/src/tests/rotation.rs:74-80,114-118`.
Value-free contrast: `crates/integration-tests/src/tests/client_access_override.rs:212-221`.
Owner: `docs/testing.md:269` and `AGENTS.md` confidentiality guidance.

## Recorded observations

The original harness specialist inspected ordinary unwrapped equality operands and the diagnostic consequence of `assert_eq!` failure.
The coordinator explicitly retained T-08 as the specialist's precise inspected-test finding rather than claiming an independently observed disclosure.
The final claim/evidence join states that no failing secret-bearing assertion was executed and browser failure artifacts were kept private and not inspected or published.

Prior passing suites are not failure-output tests.
No fixture values, raw logs, resource bytes or captured images are reproduced here.

## Reproduction or verification procedure

Source inspection, not a deliberately induced leak:

1. Identify operand types at the listed comparisons and whether redaction survives conversion.
2. Determine whether the assertion's failure diagnostics format complete operands.
3. Compare the value-free equality pattern at the cited control without copying any material.
4. Future authorized verification should use an isolated harmless mismatch and inspect only whether diagnostics omit operands and retain a failing result; do not publish a captured secret-bearing failure.

## Limits and missing evidence

No failing assertion, CI artifact disclosure or live-secret exposure was observed.
Synthetic fixture inspection is not production-log coverage, and no all-sinks or comprehensive redaction assurance is claimed.
A post-correction safe-mismatch witness remains missing.
No primary PKI fixture, private browser capture or live-secret file was needed, and no new test or fix was made during this migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
