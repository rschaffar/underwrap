# RF-0017 — Canonical-size expansion is reported as an internal finalize failure

**Priority:** Low · **State:** open

**Review aliases:** `F-02`, `A-03` · **Confidence:** High: real Rust parser/serializer size witness and independently inspected HTTP error conversion; no live finalize reproduction.

## Why it matters

An authorized writable client can submit within-input-cap typed JSON that expands past the canonical cap and receives internal failure rather than InvalidBlob; the daemon then classifies it incorrectly.

## Suggested fix

Map canonical reserialization size errors to the structured invalid-blob outcome, retaining all caps and rejection before storage or usage effects.

## Decision and caveat

Open, Low classification defect.
Includes A-03.
No cap relaxation, accepted writeback or fix authorization follows.
Rust confirmed 2,097,152 input bytes expanding to a rejected 2,097,205-byte canonical form.
HTTP 500 mapping and unchanged effects are source-derived, not a live handler observation.
Normal namespace writers canonicalize first.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
