# RF-0017 — Canonical-size expansion is reported as an internal finalize failure

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

An authenticated, authorized, currently writable runtime client submits decoded typed JSON at or below the complete-resource input cap, omitting defaulted sections.
In the recorded construction, schema version is 1, files contains one large ASCII key with an empty value, and env/libsecret are omitted.
Typed parsing accepts 2,097,152 input bytes and fills defaults.
Canonical serialization adds 53 bytes, making the attempted output 2,097,205 bytes.

`crates/underwrap-protocol/src/resource_blob.rs:185-205,365-386` separates initial input-size checking from serialized-output checking.
At `crates/underwrap-backend/src/runtime_api/invocations.rs:228-230`, `to_canonical_bytes()` fails with `ResourceTooLargeError` before the canonical-equality comparison.
Its unconditional `into_internal` conversion maps this expected invalid-input case to generic HTTP 500/internal (`crates/underwrap-backend/src/runtime_api/error.rs:76-88,107-110`).
The base64 HTTP envelope has enough headroom to reach the path (`crates/underwrap-backend/src/runtime_api/mod.rs:27-52,145-159`).

## Consequence and boundary

A deterministic validation failure is misreported as a backend failure, obscuring operator/client diagnosis.
A-03's additional consequence is retained: the daemon takes Internal rather than the ordinary InvalidRequest classification for InvalidBlob.
The contract distinguishes those outcomes at `crates/underwrap-protocol/src/service/runtime.rs:63-81,207-224` and `docs/backend.md:148-158`.

The size boundary still rejects the resource.
Control flow returns before persistence and usage update; no acceptance, confidentiality, env-integrity or authorization bypass is shown.
The relevant actor is an authenticated runtime client, not an unauthenticated local caller.
The normal namespace writer canonicalizes before sending, so this is not its normal output.

Ordinary permissive typed loading, omitted/defaulted fields and exact-canonical finalize rejection remain deliberate separate contracts.
Last-writer-wins, stateless completion and loaded-snapshot env checks are not expanded into transaction or concurrency findings.

## Correction and alternatives

Preserve typed canonical-size errors as structured InvalidBlob validation failures, without weakening either input/output caps or accepting omitted/noncanonical input.
Prefer the documented resource-too-large reason; if another existing InvalidBlob precedence is deliberately selected, specify and test that contract rather than leaving generic internal conversion.
Unexpected internal failures should remain distinguishable and value-free.

## Acceptance criteria

- Exercise the expansion case through the real handler/router with valid authorization and writable policy, not just the parser.
- Assert the exact chosen InvalidBlob status/code/reason and the daemon's invalid-request classification.
- Compare prior and post-request resource bytes and usage metadata; both must remain unchanged.
- Retain exact-cap canonical success, decoded over-cap rejection, small noncanonical rejection, env mismatch and policy-denial controls.
- Keep canonicalization and full-resource limits independent of the larger HTTP/base64 envelope.
  These checks are required for a future fix, not already executed against one.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
