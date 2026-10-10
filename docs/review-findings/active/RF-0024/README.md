# RF-0024 — Fixture comment describes obsolete executable-keyed identity

**Priority:** Low · **State:** open

**Review aliases:** `X-05` · **Confidence:** High from direct comment/module/assertion comparison; documentation-only defect.

## Why it matters

A VM fixture comment wrongly says wrappers sharing a binary share an identity, misleading maintenance of the isolation tests.

## Suggested fix

Explain the current (instance, wrapper) identity key and remove the false distinct-executable rationale.

## Decision and caveat

Open, Low localized wording debt.
Keep existing same-binary isolation checks; no behavioral change is selected.
Production identity generation is pair-keyed.
No runtime isolation failure or need for a target-identity redesign was demonstrated.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
