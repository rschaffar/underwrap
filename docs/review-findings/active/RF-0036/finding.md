# RF-0036 — Security describes the wrong CSRF token-binding mechanism

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`docs/security.md:135` describes synchronizer-token CSRF.
A reader can reasonably infer that the submitted token is compared with per-session server-side synchronizer state.
The actual application configures a separate `__Host-underwrap-csrf` cookie and `X-CSRF-Token` HMAC header (`crates/underwrap-backend/src/admin_api/csrf.rs:53-83,103-155`).
CSRF and authentication/session layers are assembled independently (`crates/underwrap-backend/src/admin_api/mod.rs:105-147`).

Locked axum_csrf 0.11.0 obtains a token from its encrypted cookie and verifies the submitted header as an HMAC of that token without accessing the login session store.
Protected mutations still require an authenticated session separately.

## Consequence and boundary

The normative owner describes the wrong binding mechanism, which can mislead security maintenance, dependency replacement and interpretation of session/CSRF tests.
This is Low documentation debt.
Host-only/Secure/HttpOnly/SameSite attributes and HMAC verification are not shown bypassable by the terminology mismatch.

### Contextual qualification: CSRF lifetime

CSRF uses locked axum_csrf 0.11.0's six-hour default, not `session_lifetime_secs` as `crates/underwrap-backend/src/runtime_state.rs:82-85` suggests.
This is distinct from the terminology finding and was independently retained during synthesis.
No bypass or approved cookie-lifetime behavior change follows; it is not a new finding or accepted vulnerability.
Session inactivity failure is separately [RF-0004](../../archived/RF-0004/README.md).

## Correction and alternatives

Describe a cookie-bound encrypted-token/HMAC or double-submit-style construction with a separately required authenticated session; do not imply server-session synchronizer storage.
Correct or qualify the lifetime comment to match actual configuration without silently changing runtime lifetime.
A different CSRF design is an optional product/security design choice requiring separate review, not this record's implied fix.

## Acceptance criteria

- Check Security wording against the application cookie/header construction, middleware assembly and selected dependency version.
- State both independent gates and retain existing cookie security attributes; avoid claiming session storage or lifetime binding not supplied by code.
- Keep the six-hour default/comment mismatch explicit as contextual documentation debt, not a behavior-change mandate.
- If implementation changes are later chosen, separately rerun session/CSRF/browser negative controls; wording correction alone is not cryptographic assurance.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
