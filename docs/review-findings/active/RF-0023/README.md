# RF-0023 — Per-crate builds and integration runner omit --locked

**Priority:** Low · **State:** open

**Review aliases:** `X-04` · **Confidence:** High on source/emitted-command mismatch; a lock-requiring drift experiment was not executed.

## Why it matters

These build/test paths can miss manifest-versus-lock drift that the documented locked commands are intended to reject.

## Suggested fix

Preserve --locked alongside per-package selection and use the same guard in the supported integration runner.

## Decision and caveat

Open, Low lock-drift detection.
No dependency update or build-policy expansion is authorized by this record.
Nix source and vendored dependencies remain pinned.
No arbitrary network resolution, nondeterministic Nix build or observed lock rewrite is claimed.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
