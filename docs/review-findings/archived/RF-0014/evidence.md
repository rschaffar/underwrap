# RF-0014 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `T-01`.
Original review owners: `test-harness.md`, `coordinator-notes.md`, `input-ledger.md`, `report.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The harness reviewer read `crates/integration-tests/src/tests/daemon.rs:1-893` and all assigned harness support, then compared acquisitions and cleanup against `docs/testing.md:248-254`.
The coordinator independently inspected consequential child ownership and retained T-01.

## Recorded observations

The spawn-to-assertion paths listed above contain no immediate kill/reap owner or Tokio kill-on-drop setting; cleanup occurs explicitly after fallible work.
The guarded `BootstrapChildren` block at lines 225-284 is a distinct positive source control.
No OS-level failure was injected and no orphan/listener was intentionally produced.

The coordinator later ran `cargo test --locked --workspace --lib --bins -- --test-threads=4`: 942 top-level tests passed, including 114 integration tests, in 107.33 seconds.
That is an exact recorded normal-suite invocation/result, not evidence that these general failure paths cleaned up, and not a new execution by this migration.

## Reproduction or verification procedure

1. Inventory each child acquisition in the cited file and follow all exits before explicit kill/wait; inspect ownership and temporary-state lifetimes.
   This is the performed source-analysis method.
2. For a future isolated witness, use matching-tree packaged backend/daemon binaries and the populated SPA prerequisites described in `docs/testing.md:36-64`.
   Install an external supervising cleanup bound before intentionally failing a test.
3. Inject failures immediately after spawn and during use; verify each owned child is gone and state is not removed prematurely.
   Test primary-error/cleanup-error coexistence separately.
4. Keep normal successful fixtures as controls.
   The supported runner can select `tests::daemon::` after preparation, but the original report did not run that targeted failure witness.

## Limits and missing evidence

No observed orphan incident, production exploit or universal shutdown guarantee is claimed.
Runtime termination/panic-abort/cancellation remain separately bounded concerns.
Passing normal tests and good ownership in other fixtures do not close this exception or invalidate the entire suite.

## Resolution evidence

Fixed at `11dd6137279f45078b93fe0dcefe13e8ea8ee05c`, based on local `main` at `7260be4feff5c2bc70d958f50e50adf218cad994`.
The executed checks used the same Rust/fixture inputs as that implementation commit; the archival follow-up changes finding records only.
The user authorized the three-finding scope in `finding.md`, with independent acceptance and retained local feature commits, not integration or publication.

### Ownership and direct witnesses

`crates/integration-tests/src/harness/packaged_child.rs::PackagedChild` installs child/state ownership before fallible output setup.
All packaged acquisitions in `tests/daemon.rs`, including both `BootstrapChildren` slots, use it.
Standard child handles plus nonblocking capture permit borrowed synchronous/asynchronous output waits without consuming ownership or requiring runtime progress during fallback.
Fallback attempts kill and polls actual `Child::try_wait` reap; signal success alone is insufficient, and unconfirmed cleanup retains the state reference without panicking over the primary failure.

`harness::packaged_child::tests::packaged_paths_reap_on_injected_failure` passed 17 isolated points covering every acquisition, readiness/use, both bootstrap children, failed second-child acquisition and the final both-child cleanup boundary.
Each worker handshakes before failure so its supervisor acquires identity-bound pidfds; owner markers require observed reap while temporary state exists, followed by pidfd exit, closed ready listeners and released state checks.
`output_wait_cancellation_keeps_owner_and_cleanup_uncertainty_keeps_state` additionally covers returned setup/output errors, cancelled borrowed waits, signal-error/observed-reap coexistence, bounded simulated observation failure and primary-panic preservation during Drop.
The uncertainty control reaps the real child first and releases only its own scratch after independently checking pidfd exit and `waitpid` ECHILD; it does not deliberately orphan a service.

`test_support::tests::supervisor_cleans_group_even_after_witness_exits` passed in the full runner.
The supervisor uses WNOWAIT before dedicated-group cleanup and reap; its fresh-process subreaper control observes actual SIGKILL/reap of a surviving child, with independent pidfd cleanup if the group mechanism regresses.
The per-worker hard bound is 30 seconds; the nested supervisor control permits 60 seconds externally to allow its inner fallback.
This is an isolated test safety net, not a production supervisor or proof of descendant containment.

### Executed checks

Environment: x86_64-linux, pinned Nix development shell, Rust/Cargo 1.97.1.
The supported runner built/verified the matching-tree SPA index and actual packaged daemon/backend executables before testing.
No placeholder SPA or harness executable substituted for a packaged service.

| Command | Result and observed scope |
| --- | --- |
| `timeout 1800 ./crates/integration-tests/run` | Exit 0; 121 integration-crate tests passed in 25.40 seconds, including all harness, packaged normal/failure and supervisor controls; zero doctests. |
| Prepared `cargo test --locked -p integration-tests harness:: -- --nocapture` | Exit 0; 13 passed in 4.08 seconds, using the populated SPA and previously built packages with unchanged production inputs; the matching-tree full runner above covers these again. |
| `cargo clippy --locked --all-targets --workspace -- -D warnings` in the prepared shell | Exit 0; final incremental check 4.16 seconds. |
| `nix develop --command bash -c 'rustc --version; cargo --version; cargo fmt --all -- --check'` | Exit 0; pinned tool versions and Rustfmt. |
| Changed Markdown local-path/layout validation and `git diff --check` | Exit 0 before archival; repeated on the record-only follow-up, not browser link verification. |

### Earlier failures and correction limits

The first authoring-time prerequisite invocation reached Cargo but failed because the new sibling test module had not yet been created; this was an incomplete draft, not a passing test run.
An initial harness run exited 101 with 12 passes and the reservation-release nonblocking assertion failing: another cooperating fixture may legitimately acquire the shared lock immediately after release.
That exact assertion now runs in a private TMPDIR lock domain rather than being weakened or retried.
A later full iteration exited 101 with 120 passes and the bootstrap-owner post-reap flock assertion failing while tests ran in parallel.
The actual transient lock holder was not captured or identified; subsequent success does not establish its precise cause.
Both exact packaged flock controls now run in fresh single-test processes, excluding unrelated pre-exec descriptor copies as required by the observation discipline, and passed in the final full runner.

A separate read-only source review found no remaining concrete ownership, cancellation or NSS-buffer acceptance blocker after these corrections; it ran no tests and is not a security audit.
No production identity, authentication, namespace, Nix or browser behavior changed.
No sanitizer, VM or aggregate SPA check was run for this fixture-only correction.
The unchanged SPA sources and [RF-0012's recorded aggregate blocker](../../active/RF-0012/evidence.md#aggregate-blocker-and-limits) are distinct from the successful SPA build prerequisite; no aggregate pass or new explanation of that earlier failure is claimed.
Runtime abort/termination, uninterruptible kernel work, arbitrary descendants and universal cleanup remain outside these direct-child witnesses.
