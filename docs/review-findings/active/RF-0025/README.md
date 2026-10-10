# RF-0025 — Store-parameterized permission helpers seed the default store

**Priority:** Low · **State:** open

**Review aliases:** `T-04` · **Confidence:** High: independently inspected helper call chain and masking existing scenario; fixture-only impact.

## Why it matters

Permission setup for a non-default store can create resources/pairs in the default store instead, then fail the target-store upsert.
Whole-policy replacement may already have removed target rows before that failure.

## Suggested fix

Seed the supplied store, or make the default-only contract explicit and provide a distinct store-aware helper.
Preserve backend reference checks.

## Decision and caveat

Open, Low-priority fixture API correctness defect; no production cross-store authorization flaw is established.
The existing remap scenario pre-seeds the target pair and already has the default pair, masking the mismatch.
No fresh two-store failure witness was executed.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
