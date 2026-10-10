# RF-0034 — Rotation dialog understates shared lifecycle serialization

**Priority:** Low · **State:** open

**Review aliases:** `W-09` · **Confidence:** High for UI/maintained-contract comparison; no new concurrency execution.

## Why it matters

The rotation dialog says serialization is only against another rotation, misdescribing the shared store lifecycle coordination.

## Suggested fix

Describe the shared lifecycle mutex accurately while preserving the distinct warning that ordinary I/O is not drained.

## Decision and caveat

Open, Low-priority wording correction.
Do not redesign rotation or infer absent backend locking from inaccurate copy.
Quiescence, possible concurrent-I/O failures, retained generations and external key custody remain intentional requirements.
No lost lifecycle lock or newly observed data loss is shown.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
