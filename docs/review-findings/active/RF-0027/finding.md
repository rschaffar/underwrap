# RF-0027 — UsageSignal timestamp tests cannot distinguish finalize from open

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`usage_signal_bumps_last_used` at `crates/integration-tests/src/tests/wrapper_writeback.rs:680-699` skips Checkout with a comment that this isolates UsageSignal's timestamp effect.
However, `open_run` at lines 46-72 first sends `WrapperRequest::Read`, receives ReadOk and the resource-channel descriptor.
That successful invocation open already updates pair usage metadata before the later channel verb.

The final assertion checks only `last_used_at.is_some()`.
The rejected-Checkin followed by UsageSignal case at lines 704-754 has the same issue: it opens successfully, later observes an Unauthorized Checkin and an accepted UsageSignal, then checks that some timestamp exists.

## Consequence and boundary

If only the usage-only finalize timestamp update regresses while open continues to update it, those timestamp assertions can still pass.
`crates/underwrap-backend/src/runtime_api/invocations.rs:128-136` attempts the open-time bump; lines 250-258 attempt the finalize bump.
The tests do not independently distinguish those effects.
Related comments at `crates/integration-tests/src/tests/wrapper_writeback.rs:575-586` attribute backend accesses to channel verbs without accounting for the earlier open.

This does not erase the accepted-verb or selected denial evidence.
The nearby `checkin_keeps_last_used_at_least_at_checkout_value` assertion and comments at lines 648-674 already qualify the same-second limit; do not inflate that bounded observation into a promised second update.

`docs/backend.md:155-159` defines timestamps as best-effort metadata, at one-second resolution, not receipts for changed content.
Equal sampled seconds can produce no rewrite; wall-clock values need not be strictly monotonic.

## Correction and alternatives

Observe or control the timestamp after open and arrange a distinguishable finalize-time value, without confusing best-effort metadata with persistent content.
Alternatively, narrow the test description to overall-run timestamp presence and retain a separately scoped attribution witness if that property is required.
Preserve the channel protocol and intentional stateless/LWW finalization semantics.

## Acceptance criteria

- Demonstrate a witness that fails when only finalize's timestamp effect is omitted while open remains intact, or explicitly narrow the claim.
- Retain normal open, UsageSignal acceptance and rejected-Checkin/follow-up-UsageSignal controls.
- Account for one-second resolution and clock behavior; avoid asserting unsupported monotonicity.
- Keep content-equality and persistence assertions independent from usage metadata.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
