# RF-0026 — Readiness loops can wait about 26 minutes rather than 60 seconds

**Priority:** Low · **State:** in-progress

**Review aliases:** `T-05` · **Confidence:** High for timeout/iteration arithmetic; no deliberately stalled 26-minute run.

## Why it matters

A listening but unresponsive fixture can consume about 1,560 seconds per readiness loop.
Admin readiness retains the cooperative startup reservation, delaying other fixtures as well.

## Suggested fix

Use an elapsed budget for cancellable readiness requests, or accurately document the longer bound.
Keep cancellation-unsafe joins outside in-runtime timeouts.

## Decision and caveat

In progress, Low-priority harness timing and diagnostics defect, not production availability evidence.
The estimate includes each 500-ms request timeout and 20-ms sleep, plus scheduling overhead.
It is source arithmetic, not measured timing; an external whole-suite bound remains necessary.

The user authorized this deadline subpart of [RF-0014's bounded shared fixture task](../RF-0014/finding.md#work-ownership).
Elapsed-budget correction and stalled/prompt controls are present; final required evidence and independent archival remain pending.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
