# RF-0023 — Per-crate builds and integration runner omit --locked

[Decision summary and current metadata](README.md) · [Evidence](evidence.md)

This finding is scoped to reviewed revision `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The summary owns priority, state, confidence and any later disposition.

## Trigger and affected flow

`flake.nix:137-141` sets `cargoExtraArgs = "--package ${name}"`, replacing Crane's default that includes `--locked`.
The per-crate check phase inherits the omission.
`crates/integration-tests/run:69-70` similarly invokes `cargo test -p integration-tests` without adding `--locked`, unless the caller supplies it.

The relevant trigger is a manifest/lock discrepancy requiring a lock update that Cargo can resolve from the available dependency set.
These paths do not require rejection of that update in the same way the workspace derivations and documented locked commands do.

## Consequence and boundary

The affected property is committed-lock drift detection, not production authorization or arbitrary dependency retrieval.
Existing source-matched info/backend derivations contain build/test package selectors without `--locked`; the workspace derivation includes the guard.
Nix source/vendor inputs are still fixed, so the omission alone is not evidence of network-selected or nondeterministic Nix dependencies.

`docs/testing.md:163-179` and `CONTRIBUTING.md` establish the exact toolchain and committed lock baseline.
Passing the current consistent-lock builds does not test how a drifted input would be rejected.
No actual drift or unintended lock rewrite was observed.

## Correction and alternatives

Retain the lock guard when adding per-package arguments and make the supported runner consistent.
Preserve argument forwarding, selected package/profile behavior, artifact preparation and vendoring.
Callers can currently add `--locked` as a workaround, but this does not make the runner's default enforce the policy.
No dependency refresh or major-version change follows from this finding.

## Acceptance criteria

- Inspect emitted per-crate build/check and runner commands for `--locked` without losing their other arguments.
- In an authorized disposable external source copy, introduce a minimal lock-requiring manifest change resolvable from the existing available dependencies.
  The affected commands must reject it rather than silently update the lock.
- Pair with an unchanged-lock success control; do not change the reviewed checkout's lock as part of verification.

## Work ownership

No correction scope or implementation plan is approved by this import.
For a small authorized correction, this file can own its scope/evidence checklist.
For a larger or shared correction, link the single agreed plan here and keep this finding's acceptance criteria distinct.
Follow the [register convention](../../README.md#planning-and-completion) and [testing matrix](../../../testing.md#choose-checks-by-change).
