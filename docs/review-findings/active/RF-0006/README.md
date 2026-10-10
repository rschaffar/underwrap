# RF-0006 — Accepted alias changes leave exported D-Bus routing stale

**Priority:** Medium · **State:** open

**Review aliases:** `S-01` · **Confidence:** High: independently confirmed startup binding and mutation source chains; no fresh D-Bus wire reproduction.

## Why it matters

After an acknowledged alias retarget or clear, its exported path can still operate on the startup collection.
Newly added aliases have no exported path during that invocation.

## Suggested fix

Make alias-path routing track the accepted alias map for create, retarget, clear and collection deletion while preserving ACK-gated state and rollback.

## Decision and caveat

Open, Medium state/routing correctness.
No protocol expansion or implementation is authorized.
The valid published graph is not bypassed.
Effects are within the invocation's projection, not cross-resource disclosure or a defeat of the trusted-target model.
Source evidence awaits a real bus regression.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
