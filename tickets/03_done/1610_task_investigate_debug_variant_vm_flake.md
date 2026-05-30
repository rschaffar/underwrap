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

## Resolution (2026-05-30) — ROOT-CAUSED + FIXED

Reproduced deterministically and root-caused. **It is a too-tight test
fixture boundary, not a policy bug.** `policy::decide` is correct and
unchanged.

### How it was reproduced

Running the VM test via the prebuilt driver (`nix build
.#checks.x86_64-linux.vm-test.driver` then invoking
`result/bin/nixos-test-driver` directly, which boots a fresh VM and runs
the full `testScript` without Nix caching the result). The flake fired on
the **first** such run — because the leak-scan `find /` was also changed
to use real `-prune`s instead of `-not -path` filters, dropping the run
from many minutes to ~50s. The faster, tighter timing made the boundary
race below land in the failing window.

### Root cause

The failing assertion was section 5 ("Expiry") of
`local_debug_subcommand`, not the `Unlocked` section the original report
named — but the daemon log line is `variant=Debug … read_granted` in both
cases, so the original attribution to `Unlocked` was a misread of which
section produced it. Mechanism:

- The fixture set `until_epoch = int(time.time()) - 1` to mean "expired 1
  second ago". Concretely `1780135105` (= `09:58:25Z`).
- `policy::decide` (`crates/shared-protocol/src/policy.rs:206`) treats a
  permission as expired only when `now_epoch > until_epoch`, and
  `now_epoch = SystemTime::now().as_secs()` is **truncated to whole
  seconds** (`policy.rs:217-220`).
- The backend evaluated the `variant=Debug` request at `09:58:25.589Z`,
  i.e. epoch `1780135105.589`, whose `.as_secs()` is `1780135105`.
- `1780135105 > 1780135105` is **false** → not expired → `read_granted`
  → the debug tool ran and printed the secret → `machine.fail(...)` saw
  it succeed → assertion failure.

So a deadline only ~1s in the past collapses to **zero effective margin**
once both `int(time.time())` and `decide`'s `.as_secs()` truncate to the
same whole second. Whether it fires depends purely on sub-second timing
between the admin PUT and the runtime read (plus host↔VM clock skew),
which is exactly why it was non-deterministic and never reproduced on the
slower original runs. None of the inspected hypotheses (persistence
ordering, stale tmpfiles, dropped admin PUT) were involved; this is the
"qemu/scheduling/timing artifact" branch, made concrete.

`decide` is correct as-is: a one-second grace at the expiry of an
admin-set deadline (real `UnsafeDebugUntil` values are minutes/hours out)
is immaterial, so no production code changed.

### Fix

`nix/tests/test.py`, section 5 of `local_debug_subcommand`: widen the
expiry margin from `int(time.time()) - 1` to `int(time.time()) - 120`,
with a comment explaining the truncation/skew boundary. Two minutes is
far outside any plausible host/VM skew, so the assertion is now
deterministic.

(Drive-by in the same change: the secret-leak scan's `find /` now uses
real `-prune`s for `/nix`, `/proc`, `/sys`, `/dev` instead of `-not
-path` filters, so it no longer stat-walks the entire Nix store. Same
assertion, dramatically faster and lower-I/O.)

Evidence log preserved at `/tmp/vm-1610-REPRO.log` during the
investigation session.
