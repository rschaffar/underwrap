# RF-0020 — Cleanup dialog says YAML was removed instead of a registry record

**Priority:** Low · **State:** archived · **Resolution:** fixed

**Review aliases:** `L-03` · **Confidence:** High: UI/backend/maintained-owner comparison plus component and desktop Chromium wording checks.

## Why it matters

Cleanup guidance identifies the wrong configuration authority, potentially sending operators to YAML while the deletion actually removed a dynamic JSON registry record.

## Suggested fix

Name the removed store registry record and retain warnings that backing data and retained generations still require deliberate handling.

## Decision and caveat

Fixed in `63a68753da4436e33c040dbc01302ac5e3a7f975` after wording acceptance passed.
The dialog names removal of the store registry record and retains manual follow-up, retained-generation, backup and no-secure-erasure warnings.
Registry-only deletion is unchanged; no YAML mutation or security-boundary violation was demonstrated.
This disposition is separate from RF-0005's shell-safety acceptance.

[RF-0005's checklist](../RF-0005/finding.md#work-ownership) owns the joint implementation and verification.
This finding retains separate wording acceptance and disposition.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
