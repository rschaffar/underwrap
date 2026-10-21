# RF-0011 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `N-02`.
Original review owners: `namespace.md`, `coordinator-notes.md`, `documentation-replacement.md`, `claim-evidence-join.md`, `report.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The namespace reviewer followed `crates/underwrap-ns/src/writeback.rs:139-159` through `crates/underwrap-ns/src/main.rs:770-790,906-958`.
The coordinator independently confirmed both early-return branches.
UsageSignal's separate authorization is supported by `crates/underwrapd/src/wrapper_resource_channel.rs:221-274` and `crates/underwrap-backend/src/runtime_api/invocations.rs:166-188`.

## Recorded observations

This is source inspection, not an executed failed-follow-up audit witness.
Both branches use `?` before constructing their rejection outcome.
`run_parent` reports structured outcomes only on success and otherwise emits the general diagnostic before ordinary target-status propagation.

The inspected combined-dispatch tests provide successful UsageSignal replies; journal tests check formatting rather than this failure combination.
The inspected read-only VM case leaves ordinary access allowed.
Those controls support their narrower properties but do not cover known rejection plus failed usage reporting.
The final claim/evidence join explicitly retained this distinction.

## Reproduction or verification procedure

1. Inspect the read-only and Unauthorized branches and trace their returned values to `report_outcomes`; this reconstructs the existing source evidence without execution.
2. For a future synthetic channel regression, provide a changed projection under read-only Checkout, then reject or fail UsageSignal.
   Capture event identifiers/reasons and status only, never resource bytes.
3. Repeat with writable Checkout, Unauthorized Checkin and failed UsageSignal.
4. Pair each with a successful-UsageSignal control; verify the expected terminal requests, rejection event and target status independently.

## Limits and missing evidence

No new channel or native-journal reproduction was run by the original reviewer, coordinator or this migration for this conjunction.
Existing suite success does not establish all failure-path audit delivery.
This finding does not claim the daemon emits no diagnostics, that policy is bypassed, or that target failure is introduced by ordinary persistence errors.

## Resolution evidence

Correction revision: `ba69f78cba08e72ff673798957d2caf2a75294f4`, based on local `main` at `7260be4feff5c2bc70d958f50e50adf218cad994`.
The isolated feature is retained without integration or publication.
RF-0011 remains active and blocked on aggregate acceptance, not on a known remaining rejection-reporting failure.

### Mechanism and source review

Both rejection branches retain `usage_error` separately from their established `WritebackOutcome`.
`report_outcomes` emits the original structured reason and a separate escaped-resource usage diagnostic, independently of human-warning suppression.
`UsageReportFailure` retains only a fixed operation class or `ResourceErrorCode`, never daemon text, arbitrary error chains or an unexpected response body.
The journal destination override is compiled only for tests; production retains its fixed journal socket and fallback.

Joint source review followed daemon open parsing and original-byte cache ownership, namespace Checkout and synchronous bounded framing, sequential daemon Checkin/UsageSignal handling, runtime-client admission/transport and backend finalize reauthorization/persistence.
The inspected owners were `crates/underwrapd/src/backend_client.rs`, `crates/underwrapd/src/wrapper_flow/read.rs`, `crates/underwrapd/src/wrapper_resource_channel.rs`, `crates/underwrap-protocol/src/message/wrapper_resource.rs`, `crates/underwrap-protocol/src/ipc.rs`, `crates/underwrap-ns/src/resource_channel.rs` and `crates/underwrap-backend/src/runtime_api/invocations.rs`.
No backend policy, wire schema, framing, lifecycle gate, publication ACK, status ownership or target-status semantics changed.
An independent read-only source pass found no blocking defect in the bounded paths; the coordinating writer verified the actual staged diff.

### Executed correction checks

Checks ran on x86_64 Linux in the pinned `nix develop` environment with Rust 1.97.1 and a genuine built SPA prerequisite.
All commands below exited 0 except the explicit negative control and aggregate blocker.

| Check | Observed result |
| --- | --- |
| `cargo test --locked -p underwrap-ns rejection_survives_usage_failure_reporting_and_target_status` with reconstructed early-return `?` behavior | Exit 101: both branches fail their exact reporting assertion after denied usage, with target exit 37 still preserved. |
| `cargo test --locked -p underwrap-ns` | 139 tests pass, including 80 fresh-process reporting/status cases across both branches, five usage replies, suppression on/off, exit 37/SIGTERM and native/fallback sinks. |
| `cargo test --locked -p underwrap-ns -p underwrap-protocol -p underwrapd -p underwrap-backend` | Affected suites pass, including backend OpenAPI/reference tests with no generated-contract diff. |
| `cargo fmt -- --check` and `cargo clippy --locked --all-targets --workspace -- -D warnings` | Pass, including after final test-only assertion/guard tightening. |
| `cargo check`, `cargo test` and `cargo clippy`, each with `--locked -p underwrap-protocol --no-default-features --features local-abi` (Clippy also `-- -D warnings`) | Pass; 64 isolated protocol tests pass. |
| Both `cargo tree --locked -p underwrap-ns -e features --depth 2` and `-e normal,build --depth 2` | Select protocol `local-abi`, not `default`/`full`; the namespace crate retains direct libc. |
| Fresh `CARGO_TARGET_DIR` with `cargo check --locked -p underwrap-ns` | Pass; protocol dep-info excludes config, control, entities, policy, service and SCM_RIGHTS modules. |
| `./crates/integration-tests/run` | 114 tests pass in 32.37 seconds after prerequisite/build preparation. |
| Prepared `cargo test --locked --workspace`, using the documented SPA and packaged daemon/backend prerequisites | Pass after final test-only tightening, including the integration suite; final workspace Rustfmt and denied-warning Clippy also pass. |
| `nix build --no-link --print-out-paths .#checks.x86_64-linux.vm-test` | Full deployed suite passes, repeated on the final application snapshot, including existing staging/startup regressions and both added RF-0011 cases. |
| Python compile of `nix/tests/test.py`, local documentation destinations/layout/index checks and `git diff --check` | Pass. |

The deterministic channel tests observe the exact Checkin-denial/UsageSignal or UsageSignal-only sequence through EOF, excluding additional Checkin or retry.
They supply no successful Checkin response, compare the complete native record or exact fallback/stderr output, reject a second datagram and observe actual `run_parent` exit/signal propagation rather than only enum values.
Usage replies cover Unauthorized, response-side transport EOF, Internal, unexpected Checkout response and successful usage.
Arbitrary daemon text and unexpected response bodies are not displayed, including on assertion failure.

The deployed cases reuse the existing files write-then-sleep target and wait for its write/readiness before locking current permission.
Read-only and writable Checkout cases respectively observe `permission_readonly` and `server_rejected`, each with exactly one attributed native event, distinct safe Unauthorized usage stderr, byte-identical backend state and direct-parent SIGKILL status.
One deployed case suppresses the optional warning without suppressing rejection or usage failure.
The final VM journal contains both `PASS: RF-0011` labels and successful test-script completion.

### Aggregate blocker and limits

`nix flake check` exited 1 in `web-test`: 12 failed and 341 passed tests across nine failed and 29 passed files.
The selected failed derivation was `/nix/store/ppcprdxc4flaqq79z66qrnvynna5wm47-underwrap-web-test-0.1.0.drv`.
SPA sources are unchanged from this feature's starting revision; the failure's cause is not established and no frontend change or weakened gate is included.
This aggregate run preceded final test-only assertion/guard tightening; namespace/workspace/lint and deployed VM checks were rerun afterward, not the failed aggregate.
The related pre-existing aggregate blocker remains owned by [RF-0012](../RF-0012/README.md); this record does not update its evidence or accept its risk.

Transport EOF, Internal and unexpected usage replies are simulated channel evidence, not deployed network-failure injection.
Channel-open/send failures have fixed classifications but no dedicated fault-injection regression here.
These selected sink/request observations establish neither durable audit delivery nor exactly-once persistence, comprehensive secret omission, every sink or universal target behavior.
No RF-0011 archival disposition is asserted while aggregate acceptance remains blocked.
