# RF-0004 — Authenticated activity does not renew inactivity expiry

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

Configure a positive `admin.session_lifetime_secs`, log in and keep using protected administration at intervals shorter than that lifetime.
`crates/underwrap-backend/src/admin_api/mod.rs:106-118` selects `Expiry::OnInactivity` but leaves tower-sessions' `always_save` default false.
Ordinary valid-user resolution reads/verifies session data without modifying it; normal administration changes backend data, not necessarily the session record.

The pinned middleware saves and reissues a nonempty session only when modified or configured to always save.
Reading a value does not mark the session modified; saving advances stored expiry.
Consequently activity alone does not move the deadline, and MemoryStore rejects the record after its last effective save expires.
Even inserting identical authentication data need not count as a change, so every login should not be assumed to refresh an existing session.

The inactivity intent is explicit in `crates/underwrap-backend/src/auth.rs:38-41` and `crates/underwrap-backend/src/runtime_state.rs:82-85`, as well as the selected expiry variant.
`docs/configuration.md:207` says only session lifetime and is not independently an explicit sliding-expiry promise.

## Consequence and boundary

An active operator must reauthenticate unexpectedly, potentially interrupting administration or a key-custody workflow.
The recorded failure was server-side: the original cookie was replayed beyond its browser Max-Age, yet the backend denied it.
No unauthorized access, extended session authority or rollback of an already committed mutation follows.

This is a concrete policy/workflow mismatch, not a claim that the product promises universal availability.
Existing hardened cookie attributes and fail-closed expired-session denial remain useful controls.
The separate CSRF construction and lifetime-comment qualification are retained in [RF-0036](../RF-0036/README.md), not treated as causes of this expiry failure.

## Correction and alternatives

Refresh sessions according to the intended inactivity policy, using the middleware's refresh capability or an equivalent explicit rule, while preserving logout, password-change/deletion revocation and genuine idle expiry.
Alternatively deliberately adopt absolute lifetime and make configuration, comments and operator expectations consistent.
The record does not select that product change.

The original review also mentioned bounding expired-session/username bookkeeping and blocking Argon2/TLS admission as optional availability work.
Those are separate limit/scheduling decisions, not accepted vulnerabilities, guarantees or requirements of this fix.

## Acceptance criteria

- Under a short lifetime, successful authenticated requests at shorter intervals must continue past the initial deadline if inactivity semantics are retained.
- Then stop activity and verify denial after a genuine idle interval, including explicit replay of the old cookie.
- Check positive cookie/store renewal, not merely client cookie retention; preserve Secure, HttpOnly, SameSite Strict, path `/` and no Domain.
- Recheck logout, password change/reset, user deletion and restart revocation independently.
- If absolute expiry is intentionally chosen instead, test and document it explicitly rather than describing sliding inactivity.
  These are future checks, not passing fixed-build results.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
