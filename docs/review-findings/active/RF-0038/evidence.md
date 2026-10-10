# RF-0038 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `L-02`.
Original review owners: `store-lifecycle.md`, `substantive-inputs.md`, `documentation-replacement.md`, `report.md`, `finding-index.md`, `input-ledger.md`, `claim-evidence-join.md`, `synthesis-verification.md`, `synthesis-verification-final.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Internal best-effort cleanup: `crates/underwrap-backend/src/admin_api/stores/lifecycle/workspace.rs:9-17`; reservation disposition: `crates/underwrap-backend/src/admin_api/stores/lifecycle/migration.rs:184-207`; startup residue handling: `crates/underwrap-backend/src/store_registry.rs:209-250`.

## Recorded observations

The lifecycle reviewer identified the compound branch by source inspection.
The coordinator and final independent synthesis retained it as the sole unresolved Low candidate, outside the 37 accepted findings.
No execution produced the compound condition.

The existing `ReserveWorkspace` failpoint fails before the helper runs.
`OpenStage` and `Cleanup` phase hooks do not cover failed removal inside `create_private_directory`.
Exact-mode tests at `crates/underwrap-backend/src/admin_api/stores/lifecycle/workspace/tests.rs:1-25` concern successful mode/validation and predicate rejection, not this ownership/residue conjunction.
Startup tests at `crates/underwrap-backend/src/store_registry/tests.rs:307-384` support exact-name residue detection, not the compound producer path.

## Reproduction or verification procedure

The recorded method was source inspection, not execution.
Proposed verification, still unperformed:

1. Use a disposable store with a known source record/tree and an external or newly authorized test harness.
   Never remount or damage a real data filesystem to force the condition.
2. Add deterministic helper-level fault control that allows exclusive mkdir to succeed, fails its subsequent chmod or metadata operation, and separately fails its own cleanup removal.
   Existing phase hooks cannot substitute.
3. Invoke the reservation/migration path, retain only safe lifecycle/code/path metadata, and compare exact source/record equality.
   Establish whether the directory is owned by the attempt and empty, rather than assuming it.
4. Run the startup initializer on the same disposable registry layout.
   Compare live and restart outcomes and separately execute all no-effect, successful-cleanup and collision controls.
5. Record whether the candidate is confirmed or rejected.
   A future approved correction would need this same matrix with coherent live/startup outcomes.

## Limits and missing evidence

There is no command result, exit, timing, retained real incident or deterministic compound-failure witness to migrate.
Fault injection itself requires a separately authorized test change; this record is not permission to implement it now.
No secret-bearing residue, data loss or full service restart has been demonstrated.
No new source review or execution occurred during migration.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
