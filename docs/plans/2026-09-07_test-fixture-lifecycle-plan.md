# Test fixture lifecycle hardening plan

## Goal

Make browser and Rust integration fixtures own cleanup from the first acquired resource through partial startup, test failure, explicit shutdown, and fallback drop paths.

## Context

The current browser fixture creates its temporary directory and child process before returning cleanup ownership, does not use `finally` around Playwright fixture use, does not await exit after `SIGKILL`, and allows one HTTPS readiness probe to outlive the outer deadline.

The current Rust integration fixture starts an independent backend thread and daemon task before returning `TestDaemon`, while fallible startup and test assertions can bypass its consuming `stop` method.

The user authorized implementation in a linked worktree and requested an ordinary merge into local `main` after verification.

## Plan Relationship

No active plan overlaps this work.

The completed release-readiness plan and `docs/research/2026-09-01_workspace-validation-stability.md` own the existing cooperative backend port-reservation design, which this plan preserves.

The uncommitted code-quality workflow finding called `CQ-01` is investigation input rather than an accepted repository record or implementation authority.

## Security Invariants

This work changes test infrastructure, not production process cancellation, authorization, brokered-resource handling, or the privileged wrapper chain.

Temporary fixture state remains alive until its owned process or task has exited or released ownership.

Diagnostics do not include brokered resource values, credentials, key material, or secret-bearing fixture contents.

## Non-Goals

- Do not change production daemon or backend shutdown semantics.
- Do not add a cross-language fixture framework or dependency.
- Do not alter the cooperative port-reservation contract or claim universal loopback-port ownership.
- Do not broaden this work to unrelated code-quality findings.

## Risks

Cleanup code can hide the primary failure, deadlock a test runtime, race temporary-directory removal, or turn a bounded startup deadline into another unbounded wait.

The implementation preserves the primary startup error, keeps awaited cleanup on normal paths, uses only bounded child waits, and keeps Rust fallback cleanup synchronous only for the independent backend thread while aborting the runtime-owned daemon task.

## Plan

- [ ] Harden `web/e2e/backend.ts` and `web/e2e/fixtures.ts` so temporary state is owned immediately, partial startup invokes cleanup, Playwright use always cleans up in `finally`, process exit is observed before directory removal, and each readiness probe is bounded by the existing startup deadline.
  Acceptance evidence: focused Node lifecycle tests fail against the original helpers for the targeted ordering/deadline cases and pass after the correction; `cd web && npm run test:e2e-support` passes.
- [ ] Harden `crates/integration-tests/src/harness/mod.rs` so partial startup owns and releases the backend thread and daemon task, explicit `stop` still joins both services, fallback drop requests shutdown without racing temporary-state lifetime, and the original startup error remains authoritative.
  Acceptance evidence: focused integration-harness lifecycle tests exercise owner drop and fixture fallback cleanup; `./crates/integration-tests/run harness::tests -- --nocapture` passes.
- [ ] Update `docs/testing.md` with the bounded fixture-lifecycle evidence and its limits.
  Acceptance evidence: documentation names the exact focused commands and does not claim production shutdown, kernel, or universal process-cleanup guarantees.
- [ ] Run the complete relevant validation for browser support and Rust integration changes.
  Acceptance evidence: `cd web && npm test && npm run build && npm run test:e2e`, `./crates/integration-tests/run`, `cargo fmt -- --check`, and `cargo clippy -p integration-tests --all-targets -- -D warnings` pass in the supported development environment.
- [ ] Review the final diff for partial acquisition, cleanup error precedence, bounded waits, process exit ordering, task cancellation, temporary-state lifetime, unrelated changes, and secret-safe diagnostics.
  Acceptance evidence: `git diff --check`, exact changed-path inspection, and a bounded sensitive-pattern scan pass.

## Completion Checklist

- [ ] Every implementation and documentation task has direct or explicitly limited simulated evidence.
- [ ] No dependency, production lifecycle, protocol, security boundary, or privileged binary-chain change is present.
- [ ] The feature branch is clean and its exact tip and validation evidence are presented for approval before the required `--no-ff` merge into the exact local `main` tip.
- [ ] Durable current behavior is owned by tests and `docs/testing.md`, and this fully evidenced plan is moved to `docs/plans/archived/` with both plan indexes updated.
