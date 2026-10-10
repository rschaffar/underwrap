# RF-0016 — Malformed input cannot distinguish HTTP body-limit enforcement

**Priority:** Medium · **State:** open

**Review aliases:** `T-03` · **Confidence:** High: independent counterfactual source inspection; no cap-disabled execution.

## Why it matters

The test intended to distinguish runtime HTTP body limits also passes when the limits are absent, because its oversized bodies are invalid JSON and the assertions accept ordinary parse rejection.

## Suggested fix

Use otherwise-valid over-limit requests with meaningful below-limit controls, or distinguish the size-rejection cause from JSON parse failure.

## Decision and caveat

Open, Medium-priority regression-evidence defect in the harness, not a demonstrated production body-limit vulnerability.
Production caps are present.
Exact resource and semantic one-byte-over tests remain useful.
No cap was removed, test mutated or counterfactual run during this migration.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
