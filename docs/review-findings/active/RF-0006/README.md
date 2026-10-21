# RF-0006 — Accepted alias changes leave exported D-Bus routing stale

**Priority:** Medium · **State:** in-progress

**Review aliases:** `S-01` · **Confidence:** High: source review and actual exported-path regressions on peer sockets and a private bus.

## Why it matters

After an acknowledged alias retarget or clear, its exported path can still operate on the startup collection.
Newly added aliases have no exported path during that invocation.

## Suggested fix

Make alias-path routing track the accepted alias map for create, retarget, clear and collection deletion while preserving ACK-gated state and rollback.

## Decision and caveat

The user-authorized joint correction is owned by [RF-0006's scope/evidence checklist](finding.md#work-ownership).
Dynamic alias routing and post-ACK export reconciliation are implemented; required validation is in progress.
The valid published graph is not bypassed.
Effects are within the invocation's projection, not cross-resource disclosure or a defeat of the trusted-target model.
Matching host ACKs in the wire regressions are simulated, not backend durability evidence.
Post-ACK export failure stops the private helper without retracting the accepted generation.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
