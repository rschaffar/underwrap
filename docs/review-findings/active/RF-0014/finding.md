# RF-0014 — Packaged-process tests lack failure-path kill and reap ownership

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

Selected packaged backend/daemon tests acquire a child and then perform fallible setup, readiness, requests or assertions before explicit happy-path cleanup.
`crates/integration-tests/src/tests/daemon.rs:148-178,213-214` has an acquired backend followed by a readiness assertion before the final kill/wait.
Other affected paths include `crates/integration-tests/src/tests/daemon.rs:732-740,797-805,842-885`; the final resource comparison at line 885 precedes both child kills.
Related unguarded synchronous acquisitions occur at `crates/integration-tests/src/tests/daemon.rs:37-57,498-573`.

These Tokio commands do not enable kill-on-drop and lack an owner which kills and reaps on unwind.
Dropping a child handle alone does not terminate the process.
Conditional timeout cleanup branches address only those explicit branches, not arbitrary assertion/error exits.

## Consequence and boundary

On a regression, the test can lose child ownership while its TempDir removes files the process still expects.
A listener/process may remain active and interfere with later tests, and cleanup evidence is missing precisely on failure.
The affected asset is test isolation and trustworthy diagnostics, not a production caller-to-daemon boundary.
`docs/testing.md:248-254` requires ownership before fallible work and distinguishes normal shutdown from fallback behavior.

There are useful contrasts, not grounds for blanket test invalidation: `BootstrapChildren` at `crates/integration-tests/src/tests/daemon.rs:225-284` guards a different path; the in-process harness owns threads/tasks and shared temporary state; browser owners observe exit and retain state on failed reap.
Those mechanisms do not automatically guard unrelated raw child acquisitions.

## Correction and alternatives

Retain explicit kill-and-reap responsibility from acquisition through every ordinary failure/unwind path, with temporary state alive until the child is confirmed exited.
Preserve the primary test failure if cleanup also fails and surface cleanup uncertainty safely.
Enabling kill-on-drop alone must not be mistaken for complete reap/state-lifetime evidence.
Reuse an appropriate existing owner or add equivalent ownership without obscuring test intent.

## Acceptance criteria

- In isolated authorized tests, inject failure immediately after each affected spawn and during readiness/use, including before both-child cleanup.
- Verify identity-bound child exit/reap and listener closure before temporary state is released; distinguish signal attempts from observed exit.
- Verify primary-error preservation when cleanup fails and a normal-success control.
- Cover packaged subprocess paths, not only in-process TestDaemon owner tests; retain an external whole-test deadline.
  No failure injection or fix validation has yet been performed.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
