# RF-0016 — Malformed input cannot distinguish HTTP body-limit enforcement

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`runtime_resource_and_generic_body_extractors_keep_distinct_bounds` in `crates/integration-tests/src/tests/runtime_api.rs:953-985` posts repeated non-JSON bytes to two runtime routes.
Finalize receives one byte over `ceil(MAX_CANONICAL_RESOURCE_BYTES / 3) * 4 + 64 KiB`; wrappers registration receives one byte over 2 MiB.
Each response is checked only for HTTP 400 and code `invalid_request`.

At the reviewed 2,097,152-byte canonical limit, the computed envelope cap is 2,861,740 bytes and its test request is 2,861,741 bytes; the generic request is 2,097,153 bytes.
These are arithmetic consequences of the inspected constants, not measured throughput or executed new cases.

## Consequence and boundary

If either HTTP cap is removed or raised above the request size, the repeated non-JSON input still fails deserialization with the same accepted status/code.
The test therefore does not discriminate the behavior its name claims.
The relevant asset is regression evidence for parser-untrusted runtime bodies, not a demonstrated current exhaustion or authorization path.

Production enforcement exists in `crates/underwrap-backend/src/runtime_api/mod.rs:27-53` and `crates/underwrap-backend/src/http/responses.rs:19-55`.
Size and parse failure both map to the accepted envelope.
The exact-resource positive and semantic one-byte-over tests at `crates/integration-tests/src/tests/runtime_api.rs:877-950` are distinct useful evidence; they do not validate these HTTP extractor bounds.
Canonical-expansion error classification is separately recorded in [RF-0017](../RF-0017/README.md).

## Correction and alternatives

Make over-limit requests otherwise acceptable to the target parser and pair them with below-bound controls.
Alternatively, assert a reliably distinguishable size-rejection cause without treating ordinary malformed JSON as equivalent.
Keep canonical resource size, base64/JSON envelope size and generic-body size as separate domains rather than enlarging all limits together.

## Acceptance criteria

- Demonstrate each route's intended boundary with validly shaped inputs and appropriate acceptance/semantic controls below it.
- Show the regression witness fails when only its relevant cap is disabled or incorrectly enlarged in an authorized disposable validation.
- Retain exact-resource and semantic rejection/state-preservation controls.
- Keep diagnostics value-free and preserve production caps; no counterfactual mutation or new test is authorized by this record.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
