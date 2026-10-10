# RF-0032 — Duplicate resource rename is misreported as a network error

**Priority:** Low · **State:** open

**Review aliases:** `W-07` · **Confidence:** High for the complete local-validation/error-handling source chain; no focused UI execution.

## Why it matters

Renaming a resource to an already-listed name is rejected locally but tells the operator to check the connection.
Retrying the same name cannot fix the collision.

## Suggested fix

Report the known collision as safe field validation while retaining generic handling for unrelated exceptions.

## Decision and caveat

Open, Low-priority diagnostic defect.
Preserve duplicate rejection and server authority over concurrent changes.
The trigger sends no rename request.
No backend corruption or connectivity failure is established, and no focused rename regression was found in the reviewed inventory.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
