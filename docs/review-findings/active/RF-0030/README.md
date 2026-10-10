# RF-0030 — Migration retry policy conflicts with authoritative-reconciliation guidance

**Priority:** Low · **State:** open

**Review aliases:** `W-04` · **Confidence:** High for deliberate UI behavior and documentation mismatch; intended policy remains undecided.

## Why it matters

After an ambiguous migration failure, retry can become enabled while the Stores refresh is stalled or failed, using the cached eligible source state.
The operator procedure instead requires authoritative reconciliation first.

## Suggested fix

Choose a coherent contract: gate retry on resolved reconciliation, or explicitly document supported stale-state retry and its limits.
Retain the key and acknowledged discard.

## Decision and caveat

Open, Low-priority contract mismatch.
Existing tests deliberately expect early retry; no alternative is selected by recording this finding.
Backend same-backend, exact-entry and lifecycle checks remain authoritative.
No unsafe migration, key loss or actual network-partition behavior was demonstrated.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
