# RF-0008 — Accepted dot-only libsecret IDs normalize out of request paths

**Priority:** Medium · **State:** open

**Review aliases:** `W-02` · **Confidence:** High for validation and Request normalization; no real browser/backend route witness.

## Why it matters

An operator can create a collection or item with a dot-only identifier that later browser requests cannot address faithfully.
Reveal or deletion can fail or reach a different route shape.

## Suggested fix

Make accepted identifiers literally addressable through the supported transport, with authoritative backend validation and matching UI feedback.

## Decision and caveat

Open, Medium-priority workflow finding.
Coordinate validation with the separate publication-identifier finding without merging away this consequence.
The executed probe intercepted fetch and made no network requests.
It did not demonstrate unauthorized deletion, an authorization bypass, or complete wrapper startup.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
