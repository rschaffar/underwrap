# Task: Investigate the one-off "debug allowed under Unlocked" VM-test result

Part of: hardening follow-up (post-2026-05 epics). Security-relevant.

## Problem

While bringing the VM test to green for ticket `1570`, one VM run
(build #4 in the session log) had `uw-local debug test-echo` **succeed**
under an `Unlocked` permission — i.e. the backend returned `read_granted`
for a `variant = Debug` request that the policy matrix says must be
`Deny { not-unsafe-debug }`. The daemon log for that run showed
`wrapper read request wrapper=test-echo variant=Debug` immediately
followed by `read_granted`.

It **never reproduced** across three subsequent fresh VM runs, and:

- the shared `policy::decide(Unlocked, Debug)` unit tests pass,
- a new deterministic end-to-end integration test
  (`integration-tests::wrapper_permissions::debug_variant_denied_under_unlocked_allowed_under_unsafe_debug`)
  drives the full daemon→backend mTLS path with `variant=Debug` and
  correctly gets `not-unsafe-debug`,
- the backend reads the policy document fresh per request (no cache
  layer between the admin PUT and the runtime read),
- the daemon threads `variant` straight through to
  `backend.open_invocation` and the backend handler passes `body.variant`
  to `decide`.

So by code inspection the gate is correct, and the one-off could not be
root-caused. A non-deterministic *allow* of the unsafe-debug path is
serious enough to warrant a deliberate investigation rather than leaving
it as "probably a flake".

## What to do

- Re-run `nix build .#checks.x86_64-linux.vm-test` several times (e.g. 10×)
  to estimate whether the anomaly recurs at all. If it never recurs,
  capture that in the ticket and downgrade.
- If it recurs, capture the full daemon + backend journals for that run
  (the `local_debug_subcommand` section already logs `variant=` lines;
  add a one-shot dump of the stored permission via the
  `wrapper-permissions-view` endpoint right before the first debug call —
  there is a removed diagnostic in the git history of `test.py` to crib
  from).
- Hypotheses to rule in/out: a write/read ordering edge in the backend's
  document persistence under VM I/O latency; a stale on-disk policy from
  tmpfiles staging; an admin-session/CSRF edge that silently dropped the
  `grant_local_unlocked` PUT (it would not have, since `admin_api` raises
  on non-2xx — verify that); a qemu/scheduling artifact.
- Consider adding a backend-side debug assertion or an explicit log of
  the `(mode, variant) → decision` at `resolve_invocation` so any future
  recurrence is self-explaining.

## Acceptance

- Either a documented root cause + fix, or documented evidence (N clean
  runs) that the anomaly does not reproduce, recorded on this ticket.
- The deterministic integration test remains green.
