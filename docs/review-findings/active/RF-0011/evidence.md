# RF-0011 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `N-02`.
Original review owners: `namespace.md`, `coordinator-notes.md`, `documentation-replacement.md`, `claim-evidence-join.md`, `report.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The namespace reviewer followed `crates/underwrap-ns/src/writeback.rs:139-159` through `crates/underwrap-ns/src/main.rs:770-790,906-958`.
The coordinator independently confirmed both early-return branches.
UsageSignal's separate authorization is supported by `crates/underwrapd/src/wrapper_resource_channel.rs:221-274` and `crates/underwrap-backend/src/runtime_api/invocations.rs:166-188`.

## Recorded observations

This is source inspection, not an executed failed-follow-up audit witness.
Both branches use `?` before constructing their rejection outcome.
`run_parent` reports structured outcomes only on success and otherwise emits the general diagnostic before ordinary target-status propagation.

The inspected combined-dispatch tests provide successful UsageSignal replies; journal tests check formatting rather than this failure combination.
The inspected read-only VM case leaves ordinary access allowed.
Those controls support their narrower properties but do not cover known rejection plus failed usage reporting.
The final claim/evidence join explicitly retained this distinction.

## Reproduction or verification procedure

1. Inspect the read-only and Unauthorized branches and trace their returned values to `report_outcomes`; this reconstructs the existing source evidence without execution.
2. For a future synthetic channel regression, provide a changed projection under read-only Checkout, then reject or fail UsageSignal.
   Capture event identifiers/reasons and status only, never resource bytes.
3. Repeat with writable Checkout, Unauthorized Checkin and failed UsageSignal.
4. Pair each with a successful-UsageSignal control; verify the expected terminal requests, rejection event and target status independently.

## Limits and missing evidence

No new channel or native-journal reproduction was run by the original reviewer, coordinator or this migration for this conjunction.
Existing suite success does not establish all failure-path audit delivery.
This finding does not claim the daemon emits no diagnostics, that policy is bypassed, or that target failure is introduced by ordinary persistence errors.

## Resolution evidence

Unresolved.
Append the disposition, relevant revision, actual checks and remaining limits when authorized work or verification settles this finding.
Preserve the discovery evidence above rather than replacing it with a retrospective success claim.
