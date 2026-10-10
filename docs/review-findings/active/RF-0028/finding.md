# RF-0028 — Body-impersonation test sends no competing body identity

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`open_uses_principal_from_cert_not_request_body` at `crates/integration-tests/src/tests/runtime_api.rs:1054-1099` grants permission to a different client instance, connects using the fixture's ordinary certificate, and expects Deny with reason `no-permission`.
Its comment describes attempted impersonation through the request body.

The request actually goes through `direct_open` at lines 59-81.
That helper serializes only `wrapper_name`, `resource_name` and `variant` in `RuntimeWrapperInvocationOpenRequest`.
It supplies no claimed `client_instance`, principal, or store field competing with the certificate-derived authority.

## Consequence and boundary

A future handler could incorrectly honor an optional identity/store override when present while still using the certificate when the field is absent.
This existing test would exercise only the absent-field path and could remain green.
That counterfactual identifies an evidence gap, not a claim that such an override exists now.

The current assertion usefully proves the selected normal request does not inherit another principal's permission.
Retain that result.
The boundary being assessed is authenticated runtime-client input toward backend resource authority: `docs/security.md:121-125` and `docs/backend.md:127-137` require enrolled-certificate-derived principal/store, not request-body authority.
Current routing at `crates/underwrap-backend/src/runtime_api/mod.rs:80-144` resolves the peer principal/store separately before calling the handler.

## Correction and alternatives

Rename or rewrite the comment to describe ordinary cross-principal isolation accurately, or preserve that control and introduce authenticated raw-JSON requests carrying explicit competing identity/store fields.
Unknown fields may be rejected or ignored according to the chosen parser contract; neither outcome may grant the other principal's authority.
Do not change production authorization to accommodate the test.

## Acceptance criteria

- Preserve the current no-permission result for the ordinary unprivileged credential.
- Use explicit competing fields and verify the result remains certificate-bound, with a matching authorized alternate-credential positive control.
- Distinguish malformed/request rejection from a genuinely exercised authorization decision.
- Show sensitivity to the intended optional-override regression, or narrow the recorded claim instead of advertising untested malicious-body resistance.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
