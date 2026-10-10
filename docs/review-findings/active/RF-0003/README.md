# RF-0003 — Structured admin libsecret identifiers fail initial publication

**Priority:** Medium · **State:** open

**Review aliases:** `F-01`, `A-01`, `S-02`, `W-01` · **Confidence:** High: independent producer-to-consumer traces plus real HTTPS creation and actual Rust strict-validator positive/negative controls; full startup not executed.

## Why it matters

Default item creation succeeds but makes the resource unusable by libsecret-enabled wrappers, including unrelated existing items.
Accepted punctuated collection/alias identifiers cause the same incompatibility.

## Suggested fix

Align new admin/API/UI identifier generation and admission with strict publication rules; preserve the host validator and repair access to existing invalid entries.

## Decision and caveat

Open, Medium scoped workflow correctness.
Includes A-01, S-02 and W-01; the original W-01 High proposal is narrowed without reducing confidence.
No fix is authorized.
Actual HTTPS producer and actual Rust validator were exercised separately, not complete namespace startup.
Arbitrary stored data need not be universally projectable; no disclosure or authorization bypass is shown.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
