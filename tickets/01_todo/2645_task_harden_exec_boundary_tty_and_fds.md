# Task: Harden the exec boundary the uid switch creates — terminal injection + fd leaks

Parent: `2600_story_target_uid_isolation`
Depends on: `2630_task_ns_setresuid_target_uid`
Blocks: `2660_task_comprehensive_isolation_vm_test`

(Inserted between 2640 and 2650 in processing order — number `2645` so it sorts
correctly; it must land with the uid switch, before production flips.)

## Why this is *required* by the epic, not optional

Until 2630 the target runs as the caller's uid, so the caller and target are the
**same trust domain** — a target that injects into the caller's shell, or
inherits a runtime fd, gains nothing it didn't already have. The moment 2630
gives the target a **different real uid X**, that changes: the caller (uid 1700)
becomes a *more-privileged* peer, and any channel from the uid-X target back to
the uid-1700 caller is now a **privilege escalation**. Two such channels are
created by the boundary and are not yet closed. Research basis: bubblewrap
CVE-2017-5226 (TIOCSTI), runc CVE-2024-21626 "Leaky Vessels" (leaked-fd escape).

## Threat 1 — controlling-terminal input injection (TIOCSTI / TIOCLINUX)

The wrapper is launched from the user's shell, so the target inherits the
caller's controlling terminal (fd 0/1/2 = the caller's pts, and
`current->signal->tty` = that pts). `underwrap-ns` **never calls `setsid()`**
(confirmed: no `setsid`/tty handling anywhere in the tree). A malicious uid-X
target can therefore `ioctl(0, TIOCSTI, &byte)` to push characters into the
terminal's input queue; after the target exits, the injected command line runs in
the caller's shell **as uid 1700** — a clean X→caller escalation.

Kernel gate (`drivers/tty/tty_io.c:2272-2294`): TIOCSTI succeeds only if
`current->signal->tty == tty` **or** the caller has `CAP_SYS_ADMIN`
(`:2280`); on `CONFIG_LEGACY_TIOCSTI=n` kernels it needs `CAP_SYS_ADMIN`
unconditionally (`:2277`). So **`setsid()` (which sets `current->signal->tty =
NULL`) makes TIOCSTI against the caller's terminal return `EPERM`** for the
capless target, even though the target still holds the inherited pts fd. After
`setsid()` the target also cannot re-acquire the caller's pts as a controlling
terminal (it already belongs to the caller's session). `TIOCLINUX` paste-injection
is virtual-console-only (`/dev/ttyN`, not `/dev/pts/N`) and on ≥6.7 also needs
`CAP_SYS_ADMIN`, so it is not a vector for the normal pty case — but deny it too.

### Fix
- **`setsid()` in the target child before exec** (sever the controlling
  terminal). Baseline, portable, uid-independent. This is exactly bubblewrap's
  CVE-2017-5226 fix.
- **seccomp-deny `TIOCSTI` and `TIOCLINUX`** ioctls in the target's filter as
  defense-in-depth (covers legacy kernels / any CAP_SYS_ADMIN edge; the cmd
  number is in a register so it filters cleanly). The target already runs under
  `no_new_privs`, so installing a seccomp filter right before exec is clean.
- **Interactive targets:** `setsid()` leaves the target with *no* controlling
  tty, so programs that open `/dev/tty` for a prompt (sudo-style) get `ENXIO`.
  Most CLI tools (incl. `aws-sso`, `gh`) read/write fd 0/1/2 fine without a
  controlling tty. For targets that genuinely need one, allocate a **fresh pty
  inside the namespace**, make the target its session leader + controlling tty,
  and run a byte relay between the caller's tty and the new pty (the
  terminal-emulator/SSH model). Scope: ship `setsid()` + seccomp now; add the
  pty-relay only when a real wrapper needs an interactive controlling tty. Gate
  any pty-relay behind a per-wrapper opt-in.

## Threat 2 — file-descriptor leaks into the target (Leaky Vessels class)

CVE-2024-21626: a runtime fd held in the host/privileged context that survives
into the sandboxed process is reachable via `/proc/self/fd/N` and re-opens the
underlying object *in its original namespace*, bypassing the sandbox. Today
`underwrap-ns` closes **only** the resource channel (fd 20,
`close_channel_fd_for_exec`) before exec — there is no general sweep. The uid
work and 2640 add **new** privileged fds that must not leak:

- the idmap **userns fd** (`/proc/<helper>/ns/user`) and the idmap helper's other
  fds (2640),
- the `ExecBundle` pipe (fd 3),
- the daemon/secretsd/dbus fds (these belong to the libsecret helpers, not the
  final target).

### Fix
- Open every runtime-internal fd with `O_CLOEXEC` from creation.
- Immediately before `cmd.exec()` of the target, **`close_range(3, ~0u,
  CLOSE_RANGE_CLOEXEC)`** (or an explicit close of every non-stdio fd) so the
  target inherits *only* stdio (and, in the libsecret case, only what the helper
  setup intends — verify the *final target* gets none of the channel/userns/pipe
  fds). Treat `/proc/self/fd/*` as an escape surface, per the research.
- The idmap-userns fd (2640) must be closed before the target exec specifically;
  it is needed only for `mount_setattr` in the cap stage.

## Acceptance

- **TIOCSTI (VM test, `test.py`):** a fixture target on the isolated instance
  attempts `ioctl(0, TIOCSTI, ...)` to inject a sentinel command; assert the
  ioctl returns `EPERM` (or `EIO`) **and** that the sentinel never executes in
  the launching shell (run the wrapper from a shell whose history/marker file is
  checked afterward). Contrast: document that pre-`setsid` it would have run.
- **fd hygiene (VM test):** read the isolated target's `/proc/<pid>/fd` **as
  root** and assert it contains only the expected stdio (no `ns/user`, no pipe,
  no channel socket, no dbus fd). For a libsecret wrapper, assert the **final
  target** has no leaked helper fds.
- `setsid()` confirmed (target is a session leader with no controlling tty:
  `/proc/<pid>/stat` session field == its own pid, tty_nr == 0).
- Existing interactive/non-interactive wrapper scenarios still run; libsecret +
  files-slot flows still pass.
- `cargo test -p underwrap-ns`, `cargo fmt --check`, `nix build .#<vm test attr>`
  green.

## Out of scope

- Full interactive pty-relay (only `setsid()` + seccomp ship here unless a
  wrapper needs a controlling tty).
- Terminal-emulator escape-sequence injection via fd 1 output (a terminal-emulator
  concern, not underwrap's boundary).
