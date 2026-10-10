# RF-0028 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `T-07`.
Original review owners: `test-harness.md`, `coordinator-notes.md`, `report.md`, `finding-index.md`, `claim-evidence-join.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Named test: `crates/integration-tests/src/tests/runtime_api.rs:1054-1099`.
Actual serialization: lines 59-81.
Normative authority: `docs/security.md:121-125` and `docs/backend.md:127-137`; current resolver/caller: `crates/underwrap-backend/src/runtime_api/mod.rs:80-144`.

## Recorded observations

Only a different principal's permission is configured; no corresponding claimed identity/store is sent in the body.
The observed test postcondition in source is Deny/`no-permission` for the ordinary credential.
The specialist ran no forged-body check and the coordinator independently inspected the helper/assertion mismatch.

The final claim/evidence join explicitly retains this weakness despite passing current suites and does not infer a principal bypass.
The coordinator's 942-test workspace result includes 114 integration tests, not a new adversarial-field matrix.

## Reproduction or verification procedure

Source inspection, not executed impersonation:

1. Read the named test's permission setup, certificate choice and expected denial.
2. Inspect the helper's typed serialized fields rather than relying on the test comment.
3. Consider the narrow optional-override counterfactual: absent fields preserve today's result, so this request cannot detect it.
4. Future authorized verification should send explicit competing fields using disposable credentials and compare both principals' outcomes without recording credentials or resource values.

## Limits and missing evidence

No forged-body request, optional-override mutation or unauthorized resource recovery was executed.
No exhaustive DTO/parser matrix or complete runtime trust-boundary proof follows from this test.
The narrower principal-isolation case remains valid evidence; it must not be discarded merely because the name overstates it.
No new test or fix was implemented during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
