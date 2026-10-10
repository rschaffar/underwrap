# RF-0026 — Readiness loops can wait about 26 minutes rather than 60 seconds

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`wait_for_admin` and `wait_for_service` at `crates/integration-tests/src/harness/readiness.rs:171-223` are described as a total window of 3,000 × 20 ms, approximately 60 seconds.
Each iteration actually awaits an HTTPS request with a 500-ms timeout before sleeping 20 ms on failure.

A listener that accepts connections but stalls each TLS/header response can consume that timeout on every iteration.
The resulting loop estimate is 3,000 × (0.5 + 0.02) seconds = 1,560 seconds, or about 26 minutes, before scheduling and other overhead.
Quickly refused connections need not take that long; this is the particular stalled-response path, not a claim about every startup.

## Consequence and boundary

The misleading short bound makes fixture failures slow to diagnose.
During admin readiness, `BackendStartupReservation` retains the cooperative process-local mutex and CLOEXEC advisory lock described at `crates/integration-tests/src/harness/readiness.rs:93-169`, so other cooperating fixtures can wait too.
The polling loops also do not directly observe that the backend thread has already failed.

This is test-runner behavior, not production service availability.
The reservation only coordinates processes sharing its temporary-directory/effective-UID lock domain; it is not a universal kernel port reservation.
An elapsed HTTP budget would not prove termination of backend threads, cleanup after runtime termination, or a total bound on all setup/teardown work.

## Correction and alternatives

Use an absolute elapsed deadline for cancellable HTTP probing, with each remaining attempt constrained by the remaining budget, or explicitly document the longer worst case if it is intentional.
Preserve useful failure diagnostics and the cooperative reservation's ownership.
Do not hide cancellation-unsafe blocking joins inside a Tokio timeout to manufacture a misleading shutdown guarantee.

## Acceptance criteria

- Check both admin and enrolled-runtime probes against a listener that stalls responses, with an external hard test bound.
- Measure the chosen elapsed budget rather than merely counting sleeps, and retain a prompt-success control.
- Check failed-backend observation and reservation release within the actual fixture ownership contract.
- Retain normal awaited shutdown and accurately describe work not bounded by the readiness deadline.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
