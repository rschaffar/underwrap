# RF-0017 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `F-02`, `A-03`.
Original review owners: `formats-storage.md`, `backend-data.md`, `substantive-inputs.md`, `coordinator-notes.md`, `verify_formats.rs`, `verify-F02-canonical-growth.log`, `targeted-verification-results.json`, `report.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Primary conversion: `crates/underwrap-backend/src/runtime_api/invocations.rs:228-230`; defaults and size gates: `crates/underwrap-protocol/src/resource_blob.rs:185-205,365-386`; HTTP headroom: `crates/underwrap-backend/src/runtime_api/mod.rs:27-52,145-159`; internal conversion: `crates/underwrap-backend/src/runtime_api/error.rs:76-88,107-110`.

Existing small trailing-whitespace coverage at `crates/underwrap-backend/src/runtime_api/invocations/tests.rs:280-310` does not exercise expansion across the cap.
Inspected integration cases at `crates/integration-tests/src/tests/runtime_api.rs:876-986` cover exact canonical size and decoded oversize, not this conjunction.

## Recorded observations

Initial specialist evidence was an ASCII-only Python length model, not Rust or HTTP.
The coordinator later linked a standalone program to the current protocol rlib and called the actual parser and serializer.
It reported: typed parsing succeeded; input length 2,097,152; typed canonical-size error present; attempted length 2,097,205; maximum length 2,097,152.
That run exited 0 in 0.27 seconds.
This supersedes the earlier model-only limitation.

The coordinator independently inspected finalize's `into_internal` path.
No real finalize request, persisted-state comparison or usage comparison was executed for this new case.

## Reproduction or verification procedure

Normalized methodology, not execution during migration:

1. In a disposable harness using the current locked protocol crate, generate a minimal typed JSON input with schema version 1, one files entry with an ASCII-only key and empty value, and omitted env/libsecret sections.
   Compute the empty-key encoding overhead, then pad only that key so total input length equals `MAX_CANONICAL_RESOURCE_BYTES`.
   Do not print or retain the resource body.
2. Call `ResourceBlob::from_canonical_bytes`; then call `to_canonical_bytes`.
   Downcast the error to `ResourceTooLargeError` and record only input length, attempted length, maximum and success/error booleans.
   Expected baseline counts are 2,097,152 to 2,097,205.
3. Inspect the cited handler conversion to establish classification separately.
   For future behavioral closure, deliver the same synthetic decoded input through an authenticated writable finalize route and compare exact structured response, prior storage and usage metadata.
4. Run the distinct boundary controls listed in acceptance criteria; do not substitute malformed JSON for an otherwise valid boundary input.

## Limits and missing evidence

No live HTTP 500 or daemon error was observed for this case; those consequences follow from inspected source.
No complete resource blob or prior external probe is needed to reconstruct the count-only method.
The exact-cap and noncanonical controls are requested regression checks, not new targeted passes.
No new product execution occurred during this migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
