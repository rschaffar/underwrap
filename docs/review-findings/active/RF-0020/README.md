# RF-0020 — Cleanup dialog says YAML was removed instead of a registry record

**Priority:** Low · **State:** open

**Review aliases:** `L-03` · **Confidence:** High: UI/backend/maintained-owner comparison independently confirmed; no browser execution.

## Why it matters

Cleanup guidance identifies the wrong configuration authority, potentially sending operators to YAML while the deletion actually removed a dynamic JSON registry record.

## Suggested fix

Name the removed store registry record and retain warnings that backing data and retained generations still require deliberate handling.

## Decision and caveat

Open, Low wording correctness.
No storage or deletion behavior change is selected or authorized.
No security-boundary violation or deletion of YAML was demonstrated.
This is distinct from the unsafe shell command in the same dialog.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
