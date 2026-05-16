# Task: Bump integration-test readiness timeouts to survive sandboxed parallel runs

Standalone ticket — does not belong to a story. Follow-on from ticket 280
(`fix_flaky_integration_test_ports`), same class of flakiness.

## Background

`nix build` (and by extension `nixos-rebuild` on hosts like shame01) fails
intermittently with the integration-tests harness panicking inside
`TestDaemon::start()`:

- `tests::policy::unlocked_until_grant_denies_after_deadline` →
  "service never became ready at https://127.0.0.1:<port>"
- `tests::wrapper_approval::per_call_approval_succeeds_with_valid_ed25519_signature` →
  "admin API never became ready at http://127.0.0.1:<port>"

The failing test is not deterministic — different tests flake on different
runs. The common factor is always one of the two readiness probes.

## Root cause

`crates/integration-tests/src/harness/readiness.rs`:

- `wait_for_admin` (line 30): `for _ in 0..100 { … sleep 20ms }` → 2-second window.
- `wait_for_service` (line 47): same 100 × 20 ms = 2 s.

On a developer machine the whole 37-test integration suite finishes in
~10 s and individual tests start in 130 ms. In the Nix sandbox during a
full workspace build, many test binaries run in parallel, CPU is contested,
and rustls TLS initialization pays a first-call cost. Under those
conditions, 2 s is too tight for the backend to bind both its HTTPS service
listener and its admin listener before the probe gives up.

Ticket 280 already moved the harness off a monotonic-counter port picker
to OS-assigned ports for a related but distinct flake. The readiness
window is the next bottleneck.

## Scope

- Bump `wait_for_admin` and `wait_for_service` iteration counts so the
  total window is ~30 s (e.g. 1500 × 20 ms, or 300 × 100 ms — choose
  based on which keeps the happy-path latency reasonable). The loop
  exits as soon as the listener answers, so a longer ceiling does not
  cost time on the happy path.
- Leave `wait_for_path` (used for wrapper sockets) at its current
  shorter window for now — those flakes have not been observed.
- Update the doc comment on each function to record the chosen
  total-window value, since the next person to debug a flake will look
  there first.

## Acceptance criteria

- [ ] `nix build .#packages.x86_64-linux.underwrap` succeeds on a
      contended machine (run twice back-to-back to stress the test
      runner).
- [ ] `cargo test --workspace` still completes in roughly the same
      wall-clock time as before on an idle dev box (the longer ceiling
      must not slow the happy path).
- [ ] Doc comments on `wait_for_admin` and `wait_for_service` reflect
      the new window.

## Out of scope

- A proper readiness signal from the backend (e.g. writing a sentinel
  file or sending a notify-style ready ping) would be more robust than
  polling. If readiness keeps flaking after this bump, file a follow-up
  ticket to switch to a notify pattern instead of increasing the
  timeout further.
