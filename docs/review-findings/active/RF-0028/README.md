# RF-0028 — Body-impersonation test sends no competing body identity

**Priority:** Low · **State:** open

**Review aliases:** `T-07` · **Confidence:** High for request-helper and assertion inspection; no forged-body execution or production bypass.

## Why it matters

A test named as protection against request-body impersonation never supplies an identity or store override.
It can miss a future override that is used only when present.

## Suggested fix

Keep the current principal-isolation witness and either narrow its claim or add explicitly competing body fields with appropriate credential controls.

## Decision and caveat

Open, Low-priority security-test evidence gap; not an accepted runtime authorization vulnerability.
The actual backend remains certificate-derived in the inspected flow.
A normal typed request denied under a different principal is useful but narrower evidence than malicious-body resistance.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
