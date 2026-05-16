# Task: Replace monotonic-counter port picker with OS-picks-port-0

Standalone ticket — does not belong to a story.

## Background

Integration tests under `crates/integration-tests/tests::store_read::*`,
`tests::service_transport::*` and (anecdotally) others fail
intermittently on `cargo test --workspace` runs. The failure mode is
always one of:

- `admin API never became ready at http://127.0.0.1:<port>` (from
  `wait_for_admin` at `crates/integration-tests/src/harness/readiness.rs:28`)
- `service never became ready at https://127.0.0.1:<port>` (from
  `wait_for_service` at the same file:57)

Each failure passes deterministically on an isolated re-run, e.g.
`cargo test -p integration-tests --lib tests::store_read::concurrent_reads`.

## Root cause

`crates/integration-tests/src/harness/readiness.rs:6-12` —
`pick_unused_port` is a global monotonic counter starting at 43000.
It returns the *next* number; it never verifies the port is actually
free. Two failure modes follow:

1. **TIME_WAIT collisions across test runs.** A previous `cargo test`
   run that bound 43000, 43001, … may still hold those ports in
   TIME_WAIT when the next run starts and picks the same numbers.
2. **External occupancy.** Anything else running on the developer's
   machine (a previous dev daemon, a debugger session) using ports in
   the 43000+ band silently shadows the test.

The local helper in `crates/integration-tests/src/tests/daemon.rs:4-10`
already does the right thing: `TcpListener::bind("127.0.0.1:0")`,
read `local_addr().port()`, drop the listener. The harness should
use the same pattern.

## Scope

- Rewrite `harness::readiness::pick_unused_port` to bind port 0,
  read the OS-assigned port, drop the listener, and return that
  number. Match the existing `pick_unused_port_local` in
  `tests/daemon.rs` and fold the two helpers into one shared module
  function.
- Delete the static `NEXT_PORT` counter.
- Keep the function synchronous (the harness callers don't await on
  it today).
- Audit other reads of fixed port ranges in `crates/integration-tests/`
  for the same issue; convert any remaining hard-coded ports.

## Acceptance criteria

- [ ] `cargo test --workspace` runs cleanly five times in a row with
      no spurious "never became ready" failures on the second-or-later
      run.
- [ ] No `static AtomicU16` port counters remain in
      `crates/integration-tests/`.
- [ ] `tests/daemon.rs::pick_unused_port_local` is removed in favour
      of the shared helper.
- [ ] No regressions in the harness's per-test isolation (still one
      port per call, still works in parallel test runs).

## Out of scope

- The race between picking the port and the server binding it is
  still technically present (the OS could reuse the port between the
  two steps). In practice this is rare enough that the OS-picks
  pattern is industry-standard for test harnesses. Holding the
  listener open until the server binds would require restructuring
  the harness more invasively; leave that to a follow-up if the OS
  pick proves insufficient.
