# RF-0007 — Deleted D-Bus objects acknowledge setters without changing state

**Priority:** Medium · **State:** archived · **Resolution:** fixed

**Review aliases:** `S-03` · **Confidence:** High: post-delete wire dispatch, retained-handler and registry-reuse regressions.

## Why it matters

Deleted interfaces previously remained exported and acknowledged setters whose backing objects no longer existed.
Create/delete cycles retained obsolete interfaces.

## Disposition and caveat

Fixed in `80b904ca0167ad9c2c87183045cf1ff60feebd01`.
Accepted deletion removes item, collection, descendant and dependent-alias interfaces; retained setters reject absent state under the store lock.
Actual-dispatch tests cover all four affected setters, rejected deletion, existing-object controls, transport rollback and bounded registry reuse.
Focused, integration, lint/format and shipped-chain VM checks passed; see [separate acceptance evidence](evidence.md#rf-0007-acceptance).

Wire-test host ACKs are simulated, not backend durability receipts.
Bounded registry observations do not establish a whole-process memory bound, denial-of-service assurance or complete D-Bus conformance.
[RF-0006](../RF-0006/finding.md#work-ownership) owns the joint correction.
Review provenance remains `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
