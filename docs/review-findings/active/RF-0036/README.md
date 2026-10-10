# RF-0036 — Security describes the wrong CSRF token-binding mechanism

**Priority:** Low · **State:** open

**Review aliases:** `C-02` · **Confidence:** High: direct application and locked-dependency inspection, with final independent confirmation of the separate lifetime qualification.

## Why it matters

Calling the mechanism synchronizer-token can mislead maintainers into assuming server-session token storage or shared session lifetime; the actual construction uses a separate encrypted cookie and HMAC header.

## Suggested fix

Describe the actual cookie-bound HMAC/double-submit-style mechanism and separately required authenticated session.
Preserve the contextual lifetime-comment mismatch without selecting a behavior change.

## Decision and caveat

Open, Low documentation precision.
No CSRF replacement or cookie-lifetime change is authorized.
No CSRF or authentication bypass was established.
Existing cookie hardening, header verification and session gates remain separate controls; this was source inspection, not cryptographic or wire conformance testing.

No correction is authorized or completed by this record.
The aliases belong to the review of `01214454aee850ebe8ceef5b757b9ae61cca07aa`.

[Technical finding](finding.md) · [Evidence](evidence.md) · [Register](../../README.md)
