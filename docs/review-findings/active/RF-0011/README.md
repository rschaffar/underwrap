# RF-0011 — Failed usage reporting hides a known writeback rejection

**Priority:** Medium · **State:** blocked

**Review aliases:** `N-02` · **Confidence:** High from source/control-flow verification, deterministic regressions and deployed denied-usage witnesses.

## Why it matters

A denied or failed follow-up UsageSignal loses the specific namespace audit outcome for changes already known to be dropped or rejected.

## Suggested fix

Report the established rejection independently of the later usage-reporting result, retaining safe diagnostics and target status.

## Decision and caveat

The authorized correction retains the established rejection and carries a separate value-free usage failure through namespace reporting.
Current authorization, terminal requests and target status remain unchanged.
Scoped checks, workspace tests and the full deployed VM pass.
The broader flake gate fails 12 tests in unchanged SPA sources; its cause remains unestablished and no SPA correction is in scope.
Keep this record active pending aggregate acceptance, as with the separate blocker in [RF-0012](../RF-0012/README.md).
Persistence still fails closed; this is not a confidentiality flaw or policy bypass.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
