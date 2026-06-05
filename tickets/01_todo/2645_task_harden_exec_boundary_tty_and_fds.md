# Task: Harden the exec boundary the uid switch creates — terminal injection + fd leaks

Parent: `2600_story_target_uid_isolation`
Depends on: `2630_task_ns_setresuid_target_uid`
Blocks: `2660_task_comprehensive_isolation_vm_test`

(Inserted between 2640 and 2650 in processing order — number `2645` so it sorts
correctly; it must land with the uid switch, before production flips.)

> **Placement:** every step below (`setsid`, `close_range`, `keyctl`,
> `CLONE_NEWIPC`/`UTS`) slots into the **canonical ordering in 2630** ("the SPINE")
> at its `[2645]` tag — consult that for exact position relative to the id switch,
> `drop_caps`, and exec. This ticket details *what* each step does and *why*.

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
- *(optional, deferrable)* **seccomp-deny `TIOCSTI`/`TIOCLINUX`** as
  defense-in-depth (covers legacy kernels / any CAP_SYS_ADMIN edge). **`setsid()`
  above is the load-bearing fix** (kernel-verified); seccomp is belt-and-braces.
  The codebase has **no seccomp infrastructure today**, so this is a **dependency
  decision** (CLAUDE.md "ask first") — `seccompiler` is pure-Rust (no C dep) if we
  want it. Ship `setsid()` first; add seccomp only if we also want it for the
  broader syscall-surface reduction the 2650 userns invites. Do not block this
  ticket on it.
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

## Threat 3 — shared IPC namespace and session keyring (cheap, in-scope)

The target also shares the caller's **IPC namespace** and **session keyring**
(the session keyring is inherited across `fork`/`execve`, even for setuid
binaries — `session-keyring(7)`). So a uid-X target can enumerate host SysV
IPC / POSIX message queues (and read any world-permissive segment), and can
read/inject keys in the caller's session keyring (cached Kerberos tickets,
`request_key` creds) — a caller↔target leak the uid boundary does **not** close
(keyring access is by *possession*, and the session keyring is possessed). Both
are closed almost for free:

- Add **`CLONE_NEWIPC`** (and, cosmetically, **`CLONE_NEWUTS`**) to the existing
  `unshare(CLONE_NEWPID | CLONE_NEWNS)` in `prepare_namespace` — one flag each,
  what bubblewrap/podman do by default. (Note `CLONE_NEWIPC` does **not** isolate
  `/dev/shm` — that is the mount ns's job; a fresh `/dev/shm` tmpfs is a separate,
  optional step.)
- Call **`keyctl(KEYCTL_JOIN_SESSION_KEYRING, NULL)`** in the target child before
  exec — gives it a fresh anonymous session keyring, detaching it from the
  caller's. (The per-uid user keyring is already X's own, distinct from the
  caller's, so only the *session* keyring needs detaching. If 2650's userns is
  used, keys are isolated by the userns too — `user_namespaces(7)` — but the join
  is the cheap fix for the common 2650-off case.)

**Not in scope here (explicit):** the **network** namespace and the host X11 /
session-D-Bus / audio sockets it exposes. Those are abstract-socket-scoped to the
net ns, and closing them needs net isolation + userspace networking — see 2600's
"untrusted target" non-goal. This ticket does *not* claim to sandbox a hostile
target from the user's GUI/session; it closes the *cheap* boundary leaks.

## Acceptance

- **TIOCSTI (VM test, `test.py`):** an isolated fixture target
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
- **IPC + keyring (VM test):** the target's `/proc/<pid>/status` shows a fresh IPC
  namespace (different `ipc` ns id from the caller via `/proc/<pid>/ns/ipc`), a
  pre-created host SysV segment is **not** visible to the target (`ipcs`), and
  `keyctl_join_session_keyring` gave it a session keyring distinct from the
  caller's (`/proc/<pid>/ns/...` n/a for keyrings — assert via `keyctl show` that
  the target's `_ses` keyring is empty/anonymous, not the caller's).
- Existing interactive/non-interactive wrapper scenarios still run; libsecret +
  files-slot flows still pass.
- `cargo test -p underwrap-ns`, `cargo fmt --check`, `nix build .#<vm test attr>`
  green.

## Out of scope

- Full interactive pty-relay (only `setsid()` + seccomp ship here unless a
  wrapper needs a controlling tty).
- Terminal-emulator escape-sequence injection via fd 1 output (a terminal-emulator
  concern, not underwrap's boundary).
