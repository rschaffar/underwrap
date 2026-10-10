# RF-0027 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `T-06`.
Original review owners: `test-harness.md`, `coordinator-notes.md`, `report.md`, `finding-index.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Test/helper chain: `crates/integration-tests/src/tests/wrapper_writeback.rs:46-72,575-586,648-754`.
Production timestamp semantics: `crates/underwrap-backend/src/runtime_api/invocations.rs:64-90,128-136,250-258`; owner: `docs/backend.md:155-159`.

## Recorded observations

The direct UsageSignal test skips Checkout, not invocation open.
Its only timestamp postcondition is `is_some()`.
The follow-up test likewise has an earlier successful open and checks presence after selected verb outcomes.
Source inspection therefore supplies a counterfactual explanation independent of finalize's bump.

The specialist ran no check for this finding; the coordinator independently inspected open/helper/handler and final assertions.
The coordinator's later 942-top-level-test workspace pass, including 114 integration tests, is unchanged-source evidence, not a timestamp-attribution experiment.
No clock-controlled or finalize-update-disabled execution was recorded.

## Reproduction or verification procedure

Source inspection, not a new protocol execution:

1. Trace `open_run` through the initial Read before either UsageSignal scenario.
2. Locate the successful backend open's metadata update.
3. Compare both tests' final presence-only assertions with the separate finalize update.
4. Reason with finalize's metadata effect absent but open unchanged: a present timestamp still satisfies the assertions.
5. Future authorized verification needs a controlled prior value or clock boundary and a discriminating negative control, reporting only timestamp/decision metadata.

## Limits and missing evidence

No missing production timestamp update or persisted-content failure was observed.
Same-second equality cannot establish that no update was attempted; best-effort failures do not fail otherwise successful operations.
These harness tests drive a synthetic wrapper-side channel against daemon/backend services, not the complete privileged namespace/target lifecycle.
No new test or source mutation occurred during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
