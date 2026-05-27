# Task: Document/handle the `SO_PEERPIDFD` fdinfo fail-closed edge

Part of: hardening follow-up (post-2026-05 epics). Low priority /
likely wontfix-document.

## Problem

`peer_auth::get_peer_cred` resolves the peer PID on the
`SO_PEERPIDFD` path by parsing `/proc/self/fdinfo/<pidfd>` for the
`Pid:` line. If a kernel advertises `SO_PEERPIDFD` (so the fallback is
*not* taken) but `/proc/self/fdinfo/<pidfd>` does not expose a parseable
`Pid:` line, `pidfd_pid` returns `None` and the daemon rejects **every**
wrapper connection.

This is fail-closed (correct direction) but it is an availability
footgun: a kernel/procfs quirk would take the whole instance down rather
than degrade. It is extremely unlikely on the supported kernels
(`hardenTargetHost` already requires ≥ 6.5, where pidfd fdinfo exposes
`Pid:`), so this is mostly a "decide and document" item.

## What to do

Pick one:

1. **Document as intentional** — add a comment in `peer_auth.rs` (and a
   line in `docs/security.md`) stating that an unexpected fdinfo shape is
   treated as a hard auth failure by design, and that
   `hardenTargetHost` pins the kernel floor that guarantees the shape.
   (Default recommendation.)
2. **Fall back within the pidfd path** — if `pidfd_pid` fails, fall back
   to `SO_PEERCRED.pid` + the uid/gid cross-check (still race-tolerant
   because the pidfd is held), instead of rejecting outright. Only do
   this if option 1 feels too brittle.

Do **not** silently widen the gate.

## Acceptance

- The chosen behaviour is explicit in code comments and (if option 1)
  in `docs/security.md`.
- A unit test pins the behaviour for a malformed fdinfo input
  (`parse_fdinfo_pid` already has reaped-`Pid: -1` coverage; add the
  missing-`Pid:` case and assert the resulting decision).
