# RF-0006 — Accepted alias changes leave exported D-Bus routing stale

**Priority:** Medium · **State:** archived · **Resolution:** fixed

**Review aliases:** `S-01` · **Confidence:** High: source review and actual exported-path regressions on peer sockets and a private bus.

## Why it matters

An acknowledged alias retarget or clear previously left its exported path accessing the startup collection.
Newly added aliases lacked callable paths.

## Disposition and caveat

Fixed in `80b904ca0167ad9c2c87183045cf1ff60feebd01` with accepted-map alias resolution and post-ACK export reconciliation.
Both alias creation methods, retarget, clear, current-target deletion and rejected/transport rollback have actual-dispatch regressions.
Required focused, integration, lint/format and shipped-chain VM checks passed; see [evidence](evidence.md#resolution-evidence).
Post-ACK reconciliation failure stops the private helper without retracting its accepted generation.

Wire-test host ACKs are simulated, not backend durability receipts.
No atomic registry transaction, complete D-Bus conformance or cross-resource disclosure claim follows.
The [joint work owner](finding.md#work-ownership) preserves RF-0007's separate acceptance.
Review provenance remains `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
