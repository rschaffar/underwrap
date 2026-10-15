# RF-0003 — Structured admin libsecret identifiers fail initial publication

**Priority:** Medium · **State:** archived · **Resolution:** fixed

**Review aliases:** `F-01`, `A-01`, `S-02`, `W-01` · **Confidence:** High: producer/validator regression plus deployed VM startup and lookup.

## Why it mattered

Default admin item creation saved identifiers rejected at libsecret startup, blocking otherwise valid items in the same resource.
Accepted punctuated collection/alias identifiers caused the same incompatibility.

## Resolution

Fixed in `83a19ca2b62fbb8ccdc131fc30d365a8ffce634f`.
Admin generation/admission, SPA feedback and OpenAPI now agree with the unchanged strict publication identifier contract.
Stored-state, real HTTPS, browser and deployed VM regressions pass, including default creation through readiness and lookup.

Existing invalid entries require explicit repair; read/delete admission remains permissive, with the separate RF-0008 dot-segment addressing limitation unchanged.
Raw storage need not be universally projectable, and no disclosure or authorization bypass was demonstrated.

The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
