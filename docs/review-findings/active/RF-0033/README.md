# RF-0033 — Locked-store delete guidance claims contents were inspected

**Priority:** Low · **State:** open

**Review aliases:** `W-08` · **Confidence:** High for UI/backend branch mismatch; no focused rendered force-delete witness.

## Why it matters

A locked store's force-delete tooltip and dialog say it contains secrets even though its contents are unknown, contradicting the table's own description.

## Suggested fix

Distinguish known nonempty contents from uninspected locked contents while retaining conservative force confirmation and registry-only deletion.

## Decision and caveat

Open, Low-priority operator-guidance correction; no relaxation of delete safeguards is selected.
A locked store may actually be empty or nonempty.
This wording defect does not show unsafe deletion without force or deletion of on-disk secret data.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
