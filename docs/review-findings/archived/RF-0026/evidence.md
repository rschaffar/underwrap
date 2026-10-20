# RF-0026 — Evidence

[Decision summary](README.md) · [Technical finding](finding.md)

## Baseline and provenance

Reviewed revision: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
The observations below come from the comprehensive review on 2026-10-08, not new behavioral checks performed during this documentation import.
Review aliases: `T-05`.
Original review owners: `test-harness.md`, `coordinator-notes.md`, `report.md`, `finding-index.md`, `substantive-inputs.md`.
Those working reports are not required to use the source references, recorded results and verification procedure retained here.
No correction has been executed or verified by creating this record.

## Source evidence

Reviewed baseline: `01214454aee850ebe8ceef5b757b9ae61cca07aa`.
Loops and timeout configuration: `crates/integration-tests/src/harness/readiness.rs:171-223`; reservation ownership: lines 93-169.
`docs/testing.md:248-258` distinguishes awaited shutdown, cancellation-unsafe work, the external suite bound and cooperative port reservation.

## Recorded observations

The specialist and coordinator independently inspected the 3,000 iterations, 500-ms request timeout and 20-ms sleep.
The 1,560-second estimate is derived arithmetic, not an observed duration or hard upper bound including scheduling.
No 26-minute stall was deliberately run.

The Rust sibling test at `crates/integration-tests/src/harness/readiness/tests.rs:1-69` covers lock exclusion, expected lock error, CLOEXEC and release, not stalled-request elapsed time.
Browser support instead has an absolute deadline and an abortable request at `web/e2e/backend.ts:384-437`; its simulated stalled-probe check is not evidence for these Rust loops.

## Reproduction or verification procedure

1. Read both loop bodies and include awaited request duration, not only sleep duration.
2. Compute the all-timeouts case separately from immediate errors and early success.
3. Trace the admin reservation's lifetime through readiness.
4. Future authorized execution should use a harmless local stalled-response fixture and a prompt-response control under an external process deadline; report actual elapsed measurements and cleanup separately.

Steps 1-3 are source inspection.
Step 4 was not performed.

## Limits and missing evidence

No observed hang, production denial of service or universal fixture cleanup guarantee is claimed.
Prior passing Rust and Node suites do not fill the missing Rust stalled-probe witness.
An in-runtime timeout around cancellation-unsafe joins would not meet the maintained ownership contract.
No new server, test or dependency was created during this migration.

## Resolution evidence

Fixed independently at `11dd6137279f45078b93fe0dcefe13e8ea8ee05c` within [RF-0014's bounded shared fixture task](../RF-0014/finding.md#work-ownership).
The archival follow-up changes finding records only.

### Deadline and ownership verification

`harness/readiness.rs::wait_for_https` gives both readiness entry points one absolute 60-second deadline, constrains each request to min(500 ms, remaining budget), applies an outer cancellable request deadline and caps sleeps at the same absolute deadline.
Diagnostics retain probe kind, endpoint, elapsed budget and last HTTP status/transport classification without response bodies or credentials.
Both thread and packaged owners observe backend completion between probes; the reviewed network-bootstrap packaged path also uses shared readiness, with a separate bounded request/body assertion afterward.

`https_readiness_elapsed_stall_success_and_failed_backend_controls` passed under a fresh process's external 30-second hard bound.
It uses a shortened 240-ms budget through the exact shared helper, with real TCP listeners that leave TLS stalled; queued connections are observed rather than substituting immediate refusal or mocked time.
Measured microseconds were: admin stall 242829, runtime stall 242818, admin HTTPS success 60310, enrolled-runtime success 39433, and failed-backend observation 6249.
The prompt controls use a real enrolled fixture and a one-second probe budget, then call normal awaited `stop` without wrapping shutdown or joins in a runtime timeout.
The failed-backend control runs the real server with missing TLS files, observes its thread finishing, verifies that readiness failure does not release the caller's reservation, synchronously joins, then drops/reacquires the reservation.

`backend_startup_reservation_excludes_processes_until_drop` retains its exact cross-process EWOULDBLOCK, in-process exclusion, CLOEXEC and post-drop successful-lock assertions.
Its fresh worker uses a private TMPDIR lock domain so another cooperating fixture cannot steal the released lock before the exact assertion.
The first parallel version failed this assertion; [shared evidence](../RF-0014/evidence.md#earlier-failures-and-correction-limits) retains that result and the isolation correction rather than weakening the assertion or inferring a pass from a retry.

### Executed checks and limits

[Shared executed commands](../RF-0014/evidence.md#executed-checks) passed: prepared focused harness 13/13 in 4.08 seconds, matching-tree full integration runner 121/121 in 25.40 seconds, pinned Rustfmt and denied-warning workspace Clippy.
The runner verified the populated SPA and actual packaged backend/daemon prerequisites.
No full 60-second stalled run or 26-minute baseline run was attempted; the measured short-budget witnesses test the same deadline mechanism.
The budget does not bound synchronous certificate/client setup, backend joins, normal shutdown, total fixture lifetime, scheduling stalls or production availability.
The reservation remains cooperative, not a universal port reservation.
No aggregate SPA check was rerun; RF-0012's recorded baseline blocker remains distinct from the successful build prerequisite and these fixture checks.
