# RF-0027 — UsageSignal timestamp tests cannot distinguish finalize from open

**Priority:** Low · **State:** open

**Review aliases:** `T-06` · **Confidence:** High: independent helper/handler/assertion inspection; no finalize-update-omission experiment.

## Why it matters

Tests claiming a UsageSignal timestamp update can remain green if that update disappears, because successful open already sets the timestamp and the final assertion only checks its presence.

## Suggested fix

Separate finalize's metadata effect from the known open-time value, or narrow the test name and comment to the observation actually made.

## Decision and caveat

Open, Low-priority evidence-attribution defect.
Preserve best-effort timestamps and independent content-persistence checks.
The tests still witness accepted verbs, selected rejection behavior and a timestamp after the run.
One-second resolution and wall-clock behavior preclude treating timestamp equality as proof of no update.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
