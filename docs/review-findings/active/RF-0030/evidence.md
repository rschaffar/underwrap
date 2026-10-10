# RF-0030 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `W-04`.
Original review owners: `web.md`, `report.md`, `finding-index.md`, `input-ledger.md`, `claim-evidence-join.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Compare `web/src/pages/stores/dialogs/migrate.tsx:73-80,97-135`, `web/src/pages/stores/page.tsx:23-25,48-53`, and `web/src/lib/query.ts:16-23` with `docs/operations.md:219-223`.
Backend mitigation is `crates/underwrap-backend/src/admin_api/stores/lifecycle.rs:176-250`.

## Recorded observations

The source test titled `re-enables retained-key controls while reconciliation remains stalled` at `web/src/pages/stores.test.tsx:923-983` supplies an initial list, leaves subsequent list requests unresolved, returns an error for migration, and asserts retry and discard enabled.
The test at lines 1033-1138 separately asserts initial retry enabled, then checks missing/busy/locked rows disable it and restoration of the source enables it.
No fixture key or response material is reproduced here.

The specialist inspected those assertions rather than running them.
The coordinator subsequently recorded a passing 318-test/38-file Vitest suite, but did not resolve the policy disagreement or add a focused failed-refresh witness.
The final input disposition explicitly retains the intentional tests and the alternative of correcting the procedure instead of changing behavior.

## Reproduction or verification procedure

This is source inspection and a future verification method, not a new execution:

1. Follow `canSubmit`, background invalidation and the request's `finally` block.
2. Read the two cited tests' deferred-list behavior and eligibility assertions alongside the operator instruction.
3. For future verification, distinguish cached eligibility, pending refresh, failed refresh and a newly confirmed source row; compare both migration directions.
4. Independently retain key/discard checks and backend rejection controls while evaluating the chosen policy.

## Limits and missing evidence

The policy discrepancy remains unresolved; the register does not turn either option into an accepted solution.
A mocked error or indefinitely pending query does not establish a real lost response, network partition, successful stale retry, or key recovery.
Existing backend protections are independent mitigation, not proof that the UI complies with its stated procedure.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
