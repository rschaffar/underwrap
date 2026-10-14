# RF-0029 — Failing equality assertions can print unwrapped fixture material

**Priority:** Low · **State:** open

**Review aliases:** `T-08` · **Confidence:** High for inspected Rust diagnostic behavior; no secret-bearing failure or live disclosure was executed.

## Why it matters

Selected failing Rust test assertions can copy unwrapped resource bytes or decoded values into captured CI logs, bypassing typed wrappers' redacted Debug behavior.
[RF-0001](../../archived/RF-0001/README.md) adds value-free VM comparisons but reuses a byte-formatting namespace-test helper; include that helper and its new callers in the correction scope.

## Suggested fix

Retain exact equality while emitting only safe decision, length or representation metadata on mismatch.

## Decision and caveat

Open, Low-priority harness evidence/log-hygiene defect, not a demonstrated production-secret leak.
The specialist inspected synthetic fixture use only.
No failing secret-bearing assertion, browser capture or live-secret disclosure was observed; fixture material must not be copied into this register.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
