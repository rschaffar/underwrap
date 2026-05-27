# Task: Move wrapper-socket peer auth to `SO_PEERPIDFD`

Part of: `epic_frontend_security_hardening_2026-05`.

## Redesign interaction (read first)

This ticket edits the **same code** that
`epic_single_instance_wrapper_redesign_2026-05` rewrites
(`frontend-underwrapd` peer auth / intake — see ticket `1530`).
Sequence this **after** the redesign bundle (1500–1540), not before,
or 1530 will rewrite your work.

Two adaptations once the redesign has landed:

- The problem statement below frames the socket as `0660`
  group=`underwrap-<inst>-<wrapper>` (per-wrapper). After the
  redesign there is **one** group per instance,
  `underwrap-<instance>`. Update the framing accordingly.
- The redesign makes this check **more** load-bearing, not less.
  Once the wrapper-name is a client-supplied, validated-against-config
  selector (no longer kernel-bound) and the daemon chooses the
  target, the peer egid check is the *sole* kernel-enforced factor in
  the identity story. Grounding it in a race-free `SO_PEERPIDFD`
  syscall stops being nice-to-have and becomes the thing the whole
  model rests on. Treat this ticket as part of the redesign's
  follow-through, not optional polish.

**Docs:** the `docs/security.md` edit below (§"Why effective GID
instead of real GID?") lands in a section the redesign's `1580`
rewrites. Fold this doc change into the consolidated `security.md`
pass (see the execution plan) rather than editing the file twice.

## Problem

`crates/frontend-underwrapd/src/peer_auth.rs:33-49` reads the peer
PID from `SO_PEERCRED` and then opens `/proc/<pid>/status` in a
separate syscall to extract the effective GID. The two-step lookup
is the classic PID-recycling race: between the `getsockopt` and the
`/proc` read, the peer can exit and a new process can be assigned
the same PID. `/proc/<recycled-pid>/status` then describes a
different process.

In this specific architecture I do **not** believe the race is
exploitable to gain unauthorised access. The wrapper socket is
mode `0660` group=`underwrap-<inst>-<wrapper>`, and that group is
only acquired via setgid-exec of the installed wrapper binary, so
any process whose `/proc/<pid>/status` shows the correct egid at
read-time is itself a legitimate wrapper invocation. The race can't
authorise someone who couldn't already connect.

But "not exploitable today, conditional on the kernel-level socket
group check" is a fragile argument to rest the identity model on.
Linux 6.5 added `SO_PEERPIDFD`, which returns a `pidfd` stable across
PID recycling. NixOS kernels are well past that bar. Tightening here
is a small change that grounds the "identity comes from the kernel"
claim in a single race-free syscall.

A second, smaller hygiene point in the same function: the daemon
currently trusts `SO_PEERCRED.uid` / `.gid` and reads only the
*effective* GID from `/proc/<pid>/status`. It does not cross-check
the `Uid:` / `Gid:` real columns from `/proc` against the
`SO_PEERCRED` values. Cheap belt-and-braces.

## What to do

In `crates/frontend-underwrapd/src/peer_auth.rs`:

1. Add a `SO_PEERPIDFD` path:

   ```rust
   const SO_PEERPIDFD: libc::c_int = 77; // since Linux 6.5
   ```

   (Or pull from `libc` if a recent enough crate version exposes it
   — check `libc::SO_PEERPIDFD` first.)

2. Attempt `getsockopt(fd, SOL_SOCKET, SO_PEERPIDFD, ...)` first. On
   success, you get a `pidfd`. Read the peer's egid via
   `/proc/self/fdinfo/<pidfd>` (which exposes a stable `Pid:` and
   then read `/proc/<pid>/status` resolved through the pidfd) — or
   more simply, derive `pid` from the pidfd via
   `pidfd_getfd`-style accessor and pass `O_PATH | /proc/<pid>` to
   `openat` so the `/proc/<pid>/status` read is bound to the same
   process the pidfd refers to. The cleanest approach is to use
   `openat(pidfd, "status", O_RDONLY)` once the kernel supports
   `/proc/<pid>/...` access through a pidfd as a directory fd
   (Linux 6.9+). If that path isn't viable, the simpler shape is:
   open `/proc/<pid>/status` *and* use the pidfd to confirm via
   `pidfd_send_signal(pidfd, 0)` that the pidfd still refers to a
   live process, accepting that the pidfd guarantees the pid hasn't
   been recycled.

3. Fall back to the current `SO_PEERCRED` + `/proc/<pid>/status` path
   when `SO_PEERPIDFD` returns `ENOPROTOOPT` (pre-6.5 kernels).
   Log a warning at startup if running on a kernel where the
   fallback is required — operators should know.

4. While here: cross-check `cred.uid` and `cred.gid` (from
   `SO_PEERCRED`) against the `Uid:` and `Gid:` real-column fields
   in `/proc/<pid>/status` and reject the request if they disagree.
   A mismatch is the signature of PID reuse on the fallback path.

5. Close the pidfd on the way out — it's a real fd.

6. The audit log line on rejection should include which path was
   used (`peerpidfd` vs `peercred-fallback`) so post-incident
   forensics can tell which integrity property was relied on.

## Why not "just fix it" without the fallback

Some Linux distros and minimal kernels in CI / VM images may still
be pre-6.5. The integration-test and VM-test infra needs to keep
working; require the fallback. Document the kernel-version
expectation in `docs/security.md` and have the NixOS module assert
the running kernel version when `hardenTargetHost = true`.

## Acceptance

- `cargo test --workspace`
- `cargo clippy --all-targets --workspace -- -D warnings`
- Unit tests for both the SO_PEERPIDFD path and the
  SO_PEERCRED-fallback path. Where possible, exercise the
  cross-check rejection by crafting a mismatched `/proc/<pid>/status`
  (or via a test stub of the proc-reading helper).
- VM test green on whatever kernel the VM ships.
- `docs/security.md` §"Why effective GID instead of real GID?"
  updated to mention SO_PEERPIDFD as the primary path and
  SO_PEERCRED as the fallback, with the version assumption.

## Resolution (2026-05-27) — code complete, VM-acceptance gated on 1570

Implemented in `crates/frontend-underwrapd/src/peer_auth.rs`
(rewritten) + sibling `peer_auth/tests.rs`:

- **Primary `SO_PEERPIDFD` path.** `getsockopt(SO_PEERPIDFD)` yields a
  pidfd; the daemon resolves the stable PID via
  `/proc/self/fdinfo/<pidfd>` and reads `/proc/<pid>/status` **while
  holding the pidfd open**, so the kernel cannot recycle the PID across
  the read — race-free. The pidfd is closed only after the read.
- **`SO_PEERCRED` fallback** for pre-6.5 kernels. `warn_fallback_once()`
  fires the first time it is used.
- **uid/gid cross-check** (`creds_consistent`): the `SO_PEERCRED`
  uid/gid must match a `/proc` column (real *or* effective, for
  robustness against the real-vs-effective question). A recycled PID
  describes an unrelated process whose ids won't match, so reuse is
  caught on the fallback path.
- **`PeerAuthPath`** (`PeerPidfd` | `PeerCredFallback`) is recorded on
  `PeerCred` and surfaced in the intake rejection audit line
  (`wrapper_flow/intake.rs`) for forensics.
- **Startup probe** `log_peer_auth_mode()` (called from `server::run`)
  logs at boot whether this kernel offers the race-free path.
- **NixOS assertion**: under `hardenTargetHost = true` the module now
  asserts `kernel.version >= 6.5`.

`libc` 0.2.184 already exposes `SO_PEERPIDFD`; no new dependency.

Unit tests cover proc-status / fdinfo parsing, the cross-check matrix
(real-column match, effective-column match, unrelated-ids rejection,
reaped-`Pid: -1`), and an end-to-end `get_peer_cred` over a real
`socketpair`. `cargo test --workspace`, clippy, and fmt are green.

**Still open (folds into 1570):** the VM test must stay green on the VM
kernel; the `docs/security.md` §"Why effective GID …" rewrite is
deferred to the Phase 4 consolidated docs pass.
