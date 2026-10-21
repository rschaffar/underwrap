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

Fixed independently at `11dd6137279f45078b93fe0dcefe13e8ea8ee05c` within [RF-0014's bounded shared fixture task](../RF-0014/finding.md#work-ownership).
The archival follow-up changes finding records only.

### FFI ownership argument

`harness/builders.rs::current_group_name` captures `getegid` and calls `getgrgid_r` with a private writable `libc::group`, initialized byte buffer and result-pointer slot on every attempt.
Only successful results are inspected; null output is explicit missing-name failure, and a non-null result must equal the supplied record with the requested effective GID.
The name must be non-null and inside that same initialized allocation; bounded slice access finds its terminator without dereferencing an unbounded returned C-string pointer.
The owned UTF-8 name copy completes while record/buffer remain alive, before either can be reused or another fixture's private lookup can affect them.
Missing/error/invalid results fail setup rather than selecting a numeric, real or supplementary identity.
Only ERANGE grows by checked doubling from 16 KiB through 1 MiB; exhausted growth and allocation failure return errors.

### Executed acceptance controls

The three `harness::builders::tests` lookup tests passed in both the prepared focused harness run and matching-tree full integration runner.
Deterministic seams cover success/copy, 16/32/64-KiB growth, seven-call ERANGE exhaustion, one-call EINTR/EIO/ENOENT failures, missing result, foreign result pointer, wrong GID, null/out-of-range/one-past-end name pointers, missing terminator, empty name and non-UTF-8 name.
The concurrency control runs eight threads, each comparing 128 real effective-group lookups and starting/stopping an ordinary real backend/daemon fixture under its own Tokio runtime.
Assertions avoid formatting group names or fixture material.

[Shared executed commands and gate results](../RF-0014/evidence.md#executed-checks): prepared focused harness exit 0 (13 passed, 4.08 seconds), matching-tree full runner exit 0 (121 passed, 25.40 seconds), pinned Rustfmt and denied-warning workspace Clippy exit 0.
The full runner supplies matching SPA and packaged-service prerequisites even though NSS lookup itself is in-process.
A read-only independent source pass found no concrete remaining pointer/lifetime defect; it performed no additional execution.

### Limits

No sanitizer was run with the pinned stable toolchain, and a clean stress schedule is not proof of race freedom or every NSS implementation's correctness.
The private-buffer ownership argument, not concurrency frequency, closes the shared-storage defect.
Production NSS and peer authentication were not changed.
The shared evidence retains earlier fixture-observation failures and the separately recorded, unrerun RF-0012 SPA aggregate blocker; neither is silently converted into a pass.
