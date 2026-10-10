# RF-0011 — Failed usage reporting hides a known writeback rejection

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

After an otherwise accepted lifecycle, the target has changed its projection and either Checkout was read-only or Checkin returned Unauthorized.
In both branches, `crates/underwrap-ns/src/writeback.rs:139-159` executes `signal_usage(...)?` before constructing `DroppedReadOnly` or `RejectedReadOnly`.
UsageSignal can independently fail because current permission expired/was locked, the runtime client was disabled, a store became unavailable, or transport failed.

The error returns before the known writeback outcome reaches the caller.
`crates/underwrap-ns/src/main.rs:770-790` invokes `report_outcomes` only for `Ok(Some(outcomes))`; the error branch emits a general end-of-run writeback diagnostic.
The specific `writeback_rejected` event and reason assembled at `crates/underwrap-ns/src/main.rs:906-958` are therefore missing.
No malicious actor is needed: Checkin denial followed by UsageSignal denial is an ordinary current-policy combination.

## Consequence and boundary

The loss is in rejection audit/diagnosis, not persistence authorization.
The read-only branch still sends no Checkin, the Unauthorized branch still does not persist, and a valid target status normally propagates.
The daemon can separately log `resource_checkin_denied`, but that is not the namespace event and cannot cover the local read-only branch where no Checkin occurred.

`docs/local-protocol.md:267-276` promises the structured rejection while admitting that the subsequent usage report is reauthorized.
`docs/operations.md:484-525` distinguishes namespace reason events from daemon terminal records.
The optional human-warning suppression setting is not the cause: control flow loses the outcome before reporting.
Size-rejection diagnostics are also a separate path.

## Correction and alternatives

Retain/report the established dropped/rejected result even if the later UsageSignal fails.
Carry a separate usage-reporting error if needed; do not make usage success a prerequisite for audit or bypass policy to obtain that success.
Preserve value-free diagnostics and the existing target-status contract.
This does not call for durable delivery guarantees, retries, timestamps as persistence receipts or broader policy changes.

## Acceptance criteria

- Exercise changed read-only Checkout with both denied and transport-failed UsageSignal.
- Exercise Unauthorized Checkin followed by each of those UsageSignal failures.
- In every case observe exactly one safe namespace rejection event with the established reason, any distinct usage error, no successful persistence and unchanged target exit/signal.
- Retain successful UsageSignal controls and verify warning suppression does not suppress the structured event.
  These are required future checks, not executed fix validation.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
