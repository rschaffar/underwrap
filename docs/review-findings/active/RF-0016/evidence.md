# RF-0016 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `T-03`.
Original review owners: `test-harness.md`, `coordinator-notes.md`, `report.md`, `finding-index.md`, `claim-evidence-join.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Input and assertions: `crates/integration-tests/src/tests/runtime_api.rs:966-982`; useful neighboring controls: lines 877-950.
Enforcing extractors: `crates/underwrap-backend/src/runtime_api/mod.rs:27-53` and `crates/underwrap-backend/src/http/responses.rs:19-55`.

## Recorded observations

Both selected requests use malformed JSON and accept only the common HTTP 400/`invalid_request` result.
Source inspection establishes the alternative parse-failure explanation.
The specialist ran no suite or mutation; the coordinator independently inspected the input/assertion counterfactual.

Separately, the coordinator recorded `cargo test --locked --workspace --lib --bins -- --test-threads=4`: 942 top-level tests passed, including 114 integration tests, in 107.33 seconds.
Two nested one-test subprocess result lines were not extra top-level tests.
That unchanged-source suite pass does not make this witness discriminating.

## Reproduction or verification procedure

Source-only methodology, not a new execution:

1. Identify the two body constructions and the accepted response fields.
2. Follow each route to its body reader and JSON parser.
3. Counterfactually remove only size rejection from the reasoning: the malformed body still reaches a parse error accepted by the existing assertion.
4. Future validation must use otherwise-valid controls and, only in an authorized disposable environment, demonstrate sensitivity to removing the relevant cap.
   Matching packaged prerequisites and the externally bounded supported integration runner remain necessary for actual execution.

## Limits and missing evidence

No cap-disabled test run, memory-exhaustion test or missing production cap was observed.
Neighboring canonical-resource tests cannot substitute for envelope/generic discrimination.
Prior broad suite execution is coordinator evidence, not specialist execution or fixed-code validation; no new tests were run while drafting.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
