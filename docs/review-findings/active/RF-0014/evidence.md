# RF-0014 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `T-01`.
Original review owners: `test-harness.md`, `coordinator-notes.md`, `input-ledger.md`, `report.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The harness reviewer read `crates/integration-tests/src/tests/daemon.rs:1-893` and all assigned harness support, then compared acquisitions and cleanup against `docs/testing.md:248-254`.
The coordinator independently inspected consequential child ownership and retained T-01.

## Recorded observations

The spawn-to-assertion paths listed above contain no immediate kill/reap owner or Tokio kill-on-drop setting; cleanup occurs explicitly after fallible work.
The guarded `BootstrapChildren` block at lines 225-284 is a distinct positive source control.
No OS-level failure was injected and no orphan/listener was intentionally produced.

The coordinator later ran `cargo test --locked --workspace --lib --bins -- --test-threads=4`: 942 top-level tests passed, including 114 integration tests, in 107.33 seconds.
That is an exact recorded normal-suite invocation/result, not evidence that these general failure paths cleaned up, and not a new execution by this migration.

## Reproduction or verification procedure

1. Inventory each child acquisition in the cited file and follow all exits before explicit kill/wait; inspect ownership and temporary-state lifetimes.
   This is the performed source-analysis method.
2. For a future isolated witness, use matching-tree packaged backend/daemon binaries and the populated SPA prerequisites described in `docs/testing.md:36-64`.
   Install an external supervising cleanup bound before intentionally failing a test.
3. Inject failures immediately after spawn and during use; verify each owned child is gone and state is not removed prematurely.
   Test primary-error/cleanup-error coexistence separately.
4. Keep normal successful fixtures as controls.
   The supported runner can select `tests::daemon::` after preparation, but the original report did not run that targeted failure witness.

## Limits and missing evidence

No observed orphan incident, production exploit or universal shutdown guarantee is claimed.
Runtime termination/panic-abort/cancellation remain separately bounded concerns.
Passing normal tests and good ownership in other fixtures do not close this exception or invalidate the entire suite.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
