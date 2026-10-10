# RF-0021 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `N-03`.
Original review owners: `namespace.md`, `documentation-replacement.md`, `input-ledger.md`, `claim-evidence-join.md`, `report.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The namespace report traces initialization, generation-zero publication, Armed and final composition at the paths above.
The fresh documentation reviewer agrees that `docs/local-protocol.md:99-101` should name the initialized baseline; the final claim/evidence join accepts that clarification.

## Recorded observations

`crates/underwrap-secretsd/src/section_store/tests.rs:128-136` explicitly asserts default seeding.
`nix/tests/test.py:3467-3473` primes provider defaults before unchanged/cancellation comparisons.
Namespace publication tests use already-defined candidates rather than proving generation zero equals Checkout.
These are inspected assertions, not a fresh execution of the empty-initial-state/death conjunction.

The original reviewer had high behavioral confidence but medium confidence that “unchanged” was meant to assert Checkout equality.
The accepted final disposition resolves this as a narrow clarification, not a newly found gate vulnerability.

## Reproduction or verification procedure

1. Compare an empty or default-missing libsecret section with the result of provider initialization in the cited source/test.
2. Trace initial publication/readiness before Armed and selected-generation composition against original Checkout.
3. Review the documentation sentence using those two distinct baselines.
4. If adding the proposed regression, isolate the Armed-to-exec death window and assert only the qualified initialized-generation outcome, with failure-gate/read-only controls.

Steps 1-3 describe source verification; step 4 remains future work.

## Limits and missing evidence

No focused empty-Checkout post-Armed death test was executed for this finding.
Existing cancellation tests do not establish initial byte equality.
No arbitrary pre-exec content, broadened authorization, target execution or backend delivery guarantee follows from this clarification.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
