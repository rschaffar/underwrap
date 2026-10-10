# RF-0035 — PATH-dependent keygen instructions can select an entry wrapper

**Priority:** Low · **State:** open

**Review aliases:** `C-01` · **Confidence:** High on the conditional naming/argument mismatch; no deployed sudo-path reproduction.

## Why it matters

With an admitted keygen instance and wrapper-first command lookup, documented identity-installation commands select underwrap-exec instead of the keygen helper.

## Suggested fix

Use /run/current-system/sw/bin/underwrap-keygen explicitly in frontend operator instructions, preserving valid selectors and the existing chain.

## Decision and caveat

Open, Low conditional documentation/workflow correctness.
No naming redesign or credential operation is authorized.
Requires the stated sudo/root PATH ordering.
The demo tutorial alone does not create the collision.
No secret exposure, escalation or authorization bypass was demonstrated.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
