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

- [x] Harden `web/e2e/backend.ts` and `web/e2e/fixtures.ts` so temporary state is owned immediately, partial startup invokes cleanup, Playwright use always cleans up after success or failure, process exit is observed before directory removal, and each readiness probe is bounded by the existing startup deadline.
  Evidence: direct Node execution passed nine focused lifecycle tests covering partial acquisition, spawn failure, fixture-use failure with cleanup failure, TERM-to-KILL escalation including synchronous signal-delivery failures, unreaped-child state retention, a stalled probe deadline, and child exit during a stalled probe.
- [x] Harden `crates/integration-tests/src/harness/mod.rs` so partial startup owns and releases the backend thread and daemon task, explicit `stop` still joins both services, fallback drop requests shutdown without racing temporary-state lifetime, and the original startup error remains authoritative.
  Evidence: `./crates/integration-tests/run harness::tests -- --nocapture` passed three focused lifecycle tests, including one real backend/daemon fixture fallback drop, and the complete 110-test integration suite passed.
- [x] Update `docs/testing.md` with the bounded fixture-lifecycle evidence and its limits.
  Evidence: the focused commands, simulated child-process scope, real fixture scope, and production-shutdown and operating-system-signal nonclaims are documented.
- [x] Run the complete relevant validation for browser support and Rust integration changes.
  Evidence: `npm run test:e2e-support` passed 9 tests; `npm test` passed 192 tests; `npm run build` passed; the supported Nix-development-shell Playwright run passed 38 tests after building its documented backend-binary prerequisite; `./crates/integration-tests/run` passed 110 tests; Rustfmt and denied-warning integration-test Clippy passed.
  An earlier Playwright invocation without `target/debug/underwrap-backend` was rejected as invalid prerequisite evidence before the accepted run.
  One first exact-tip Playwright run had an existing wrappers cell-editor visibility timeout; its immediate complete rerun passed all 38 tests.
- [x] Review the final diff for partial acquisition, cleanup error precedence, bounded waits, process exit ordering, task cancellation, temporary-state lifetime, unrelated changes, and secret-safe diagnostics.
  Evidence: `git diff --check` passed; exact changed-path inspection and a bounded sensitive-pattern scan found only expected fixture identifiers and test material; three independent bounded review rounds found and drove corrections for asynchronous and synchronous signal-error handling and a Rust evidence overclaim, and the final review approved exact tip `7bdbb262df9e062dbcc66d1f9d9eb170ace17342` with no Critical, Major, or Minor findings.

## Completion Checklist

- [x] Every implementation and documentation task has direct or explicitly limited simulated evidence.
- [x] No dependency, production lifecycle, protocol, security boundary, or privileged binary-chain change is present.
- [ ] The feature branch is clean and its exact tip and validation evidence are presented for approval before the required `--no-ff` merge into the exact local `main` tip.
- [ ] Durable current behavior is owned by tests and `docs/testing.md`, and this fully evidenced plan is moved to `docs/plans/archived/` with both plan indexes updated.
