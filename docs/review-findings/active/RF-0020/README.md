# RF-0020 — Cleanup dialog says YAML was removed instead of a registry record

**Priority:** Low · **State:** in-progress

**Review aliases:** `L-03` · **Confidence:** High: UI/backend/maintained-owner comparison independently confirmed; no browser execution.

## Why it matters

Cleanup guidance identifies the wrong configuration authority, potentially sending operators to YAML while the deletion actually removed a dynamic JSON registry record.

## Suggested fix

Name the removed store registry record and retain warnings that backing data and retained generations still require deliberate handling.

## Decision and caveat

The authorized wording correction names the store registry record without changing registry-only deletion.
No security-boundary violation or deletion of YAML was demonstrated.
This is distinct from the unsafe shell command in the same dialog.

[RF-0005's checklist](../RF-0005/finding.md#work-ownership) owns the joint implementation and verification.
This finding retains separate wording acceptance and disposition.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
