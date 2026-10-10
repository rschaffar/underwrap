# RF-0021 — Post-Armed wording omits the initialized-generation baseline

**Priority:** Low · **State:** open

**Review aliases:** `N-03` · **Confidence:** High on initialization behavior; the original ambiguity concerned the meaning of “unchanged,” not gate correctness.

## Why it matters

The post-Armed qualification can imply Checkout-identical state, although validated generation zero may already contain provider defaults.

## Suggested fix

Name the fully validated initialized projection as the comparison baseline while retaining the narrow lifecycle ambiguity.

## Decision and caveat

Open, Low documentation clarification, accepted by the namespace and documentation reviews.
No behavioral redesign is selected.
No new execution-gate exploit, arbitrary pre-exec state or successful-exec guarantee is established.
Publication ACK is not a persistence receipt.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
