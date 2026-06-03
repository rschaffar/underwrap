# Story: Protect the wrapped target (and its descendants) from same-UID inspection

Source: research findings (June 2026) into `/proc/<pid>/environ` exposure of
wrapped targets. The investigation notes live in the operator's
`~/tmp/research-findings.md`; the load-bearing conclusions are reproduced in the
child tickets.

## Background — what we got wrong

underwrap's `hardenTargetHost` posture and the decisive VM test
(`local_pid_namespace_proc_isolation`) assume that `kernel.yama.ptrace_scope=2`
prevents a same-UID caller from reading a wrapped target's
`/proc/<pid>/environ`. **That assumption is false**, verified against the kernel
source (checked on v7.1-rc6; same code on v6.12):

- `/proc/<pid>/environ` is opened with `PTRACE_MODE_READ` (`environ_open` →
  `proc_mem_open`, `fs/proc/base.c:1001-1003`; `:845` adds `FSCREDS`). Yama's
  `yama_ptrace_access_check` only acts on `PTRACE_MODE_ATTACH`
  (`security/yama/yama_lsm.c:355`), so **ptrace_scope never gates `environ`**.
- **`/proc/<pid>/maps`, `smaps` and `auxv` are the same** — all opened
  `PTRACE_MODE_READ` (`fs/proc/task_mmu.c:352`, `:1546`; `fs/proc/base.c:1078`),
  so ptrace_scope does **not** gate them either. (An earlier draft of this story
  claimed `maps` was `PTRACE_MODE_ATTACH` and therefore protected — **that was
  wrong**. Only `/proc/<pid>/mem`, ptrace-attach and `process_vm_readv` are
  ATTACH-mode — `fs/proc/base.c:874`, `mm/process_vm_access.c:203` — and thus
  genuinely gated by ptrace_scope. `mem` plus the VM test's escape hatch — not
  `maps` — were doing the masking.)
- The only gate left for the READ-mode files is the target's **dumpable** flag
  (`kernel/ptrace.c:353`, `task_still_dumpable`), and a normal same-UID `execve`
  of a world-readable binary resets dumpable to `SUID_DUMP_USER=1`
  (`fs/exec.c:1210-1215`, the `euid==uid && egid==gid` branch). So
  `underwrap-ns`'s `make_process_non_dumpable()` immediately before `cmd.exec()`
  is a **no-op for the target** — the exec wipes it.

Net effect: every exec target (`aws-sso`, the shell it spawns, `dbus-daemon`) is
readable via `/proc/<pid>/{environ,maps,smaps,auxv}` by any other uid-1700
process; only `mem`/ptrace/`process_vm_readv` are blocked, and solely by
ptrace_scope=2. The non-exec'ing wrapper stages (`underwrap-exec` — non-dumpable
because it is setgid; the `underwrap-ns` supervisor and `underwrap-secretsd` —
non-dumpable via explicit `PR_SET_DUMPABLE=0`) are protected, but that protection
is the **dumpable flag**, not ptrace_scope.

## The fix — a real UID boundary

The `__ptrace_may_access` gate (which guards `environ`, `mem`, `maps`, `auxv`,
`ptrace`, `process_vm_readv`) requires the caller's uid to equal **all three** of
the target's real, effective, and saved uid (`kernel/ptrace.c:330-335`); a
mismatch returns `EPERM` *before* the dumpable check or Yama is even consulted
(`:340`). The only robust way to make a target unreadable by uid-1700 processes
is to give it a **different real uid X**. That protects the whole subtree
(children inherit the real uid) and — unlike ptrace_scope — also covers the
READ-mode files (`environ`/`maps`/`smaps`/`auxv`).

This does **not** rely on the exec becoming non-dumpable: with real=eff=saved=X
the target exec is a *normal* same-uid exec, so `begin_new_exec` resets dumpable
to `SUID_DUMP_USER` (`fs/exec.c:1210-1215`). The protection is the **uid gate**,
full stop — not the dumpable flag (see 2630).

To keep the target usable as the human user without the Option-A/-B sharp edges
(bash drops euid when euid≠ruid; `getuid()` confusion; broken home/socket
access), we layer:

- a **dedicated uid X** per instance (the protection — 2630),
- **idmapped bind mounts** of the user's `$HOME` and `$XDG_RUNTIME_DIR` so the
  uid-X target reads/writes the real user's files as that user, writes landing
  on disk owned by the real user (the convenience — 2640),
- *(optional)* a **user namespace** that relabels X ↔ the caller's uid inside,
  so `getuid()==1700` and the world looks like a normal session (cosmetic
  fidelity — "the target believes it is robert" — 2650).

The user namespace (2650) is **not** part of the protection or of file access:
the protection comes from the real uid X, and the idmapped mounts already present
home files to the uid-X target as its own (so `st_uid == getuid()` checks pass).
2650 only changes `getuid()`/displayed-name cosmetics. It is opt-in and can be
deferred for `$HOME`-env-driven targets (e.g. `aws-sso`); enable it only for
targets that branch on `getuid()` or resolve `$HOME` via
`getpwuid(getuid())->pw_dir` instead of the `HOME` env var.

This is the rootless-container model (`podman --userns=keep-id` + `:idmap`),
implemented natively in `underwrap-ns` rather than by adopting a container
runtime — the broker/secretsd/fd-channel architecture is underwrap's and stays
underwrap's; only the sandboxed-exec layer grows.

### Authority for the target uid (design rule)

`underwrap-ns` holds `cap_setuid`, so *which* uid it switches to is itself
security-critical: with that cap `setresuid(0,0,0)` succeeds and a uid-0 target
owns root files even with caps dropped. The target uid is therefore **chosen by
trusted config, anchored on kernel identity — never supplied by the wrapper and
never carried on the wire.** Concretely: `underwrap-ns` runs with `egid` = the
instance group (kernel-pinned by `underwrap-exec`'s setgid bit; unforgeable
without `cap_setgid`, which the unprivileged stage lacks), and reads the
`target_uid`/`target_gid` from the root-written instance sidecar selected by that
egid. `ExecBundle` carries no target id. This is the exact analogue of the
existing invariant *"the daemon — not the wrapper — chooses the target binary"*:
a compromised `underwrap-exec` cannot make the privileged stage switch to root,
another service, or another instance, because it can neither forge the egid nor
write `/etc`. (See 2620/2630.)

### The boundary cuts both ways — defend target → caller too

Giving the target a distinct uid X is what *creates* a privilege boundary between
the target and the caller (uid 1700). Until now the threat was one-directional
(caller reading the target). But a boundary has two sides: because the caller is
now the *more-privileged* peer, any channel from the uid-X target back to the
caller becomes an **escalation** (X → 1700) that did **not** exist when the target
ran as the caller. The epic must close these or the isolation is a half-boundary.
Enumerated (with the ticket that closes each):

- **Controlling-terminal injection (TIOCSTI/TIOCLINUX).** The target inherits the
  caller's controlling tty and can inject input that runs in the caller's shell as
  uid 1700. `underwrap-ns` does no `setsid()` today. → **2645** (setsid + seccomp).
- **Runtime fd leaks (Leaky Vessels, CVE-2024-21626).** A privileged fd inherited
  by the target (the idmap-userns fd from 2640, the ExecBundle pipe, helper fds)
  is reachable via `/proc/self/fd` and escapes the sandbox. Only fd 20 is closed
  today. → **2645** (`close_range` sweep + `O_CLOEXEC`).
- **setuid-bit planting via idmapped writeback (CVE-2023-0386 class).** The target
  can create a file that, via the idmap, lands owned by the caller (1700) with the
  setuid bit, then relies on the user running it → runs as 1700. → **2640**
  (`MOUNT_ATTR_NOSUID|NODEV` on the idmapped mounts; residual social vector
  documented).
- **Symlink-TOCTOU against the supervisor's writeback walk.** The uid-1700
  supervisor walks target-controlled staging at checkin; a target-planted symlink
  could redirect it to a 1700-owned path the target itself can't reach. → **2630**
  (walk under `setfsuid(X)` so the walk has only X authority, plus
  `openat2(RESOLVE_NO_SYMLINKS|RESOLVE_BENEATH)`).

## What this does NOT defend against (document, don't pretend)

- **root.** A caller who can `sudo` gets `CAP_SYS_PTRACE` and bypasses the uid
  gate. No same-host mechanism hides memory from root. This protects against
  *unprivileged* same-uid code (compromised/malicious processes running as the
  user), which is the realistic threat.
- **Secrets the target deliberately persists** into the user's home (e.g.
  `aws-sso` caching creds under `~/.aws`) are the user's files on disk and remain
  readable by the user. Use the libsecret/secretsd path (Option D) for secrets
  that must never hit disk.
- **User-private files outside the idmapped paths.** The idmapped-mount approach
  is *per-path*: the uid-X target gets full rw to the user's files only on the
  paths we idmap (`$HOME`, `$XDG_RUNTIME_DIR`, plus anything added to
  `idmapPaths`). World-readable system files work via the "other" bits, but a
  user-private file (mode 0600/0700, owned by the caller) **outside** an idmapped
  path is `EACCES` to uid X. This is the inherent trade-off of protection-by-
  distinct-kuid; the only way to get blanket access would be to idmap `/`
  recursively, which is invasive and fragile (every submount, procfs/sysfs
  restrictions) and is **not** done. Targets that roam the filesystem must list
  the extra paths in `idmapPaths`.
- **World-readable `/proc/<pid>` metadata.** The uid gate (`ptrace_may_access`)
  protects only the secret-bearing files — `environ`, `mem`, `maps`, `smaps`,
  `auxv`, `pagemap`, and the `fd`/`fdinfo`/`cwd`/`root`/`exe` links (so the caller
  **cannot** re-open the target's secret file or unix socket via
  `/proc/<pid>/fd` — verified `fs/proc/base.c:1853→718`, a *positive* property of
  the boundary). But `cmdline`, `comm`, `stat`, `status`, `mounts`, `mountinfo`,
  `cgroup`, `limits` are **world-readable** (`cmdline` has no ptrace check —
  `fs/proc/base.c:393`) and remain visible to the caller via the host `/proc`,
  regardless of uid. This is **not** a secret leak: underwrap secrets live in
  env/files/libsecret (all gated) and **never** in argv (existing invariant), and
  mount-path names are not secret. Hiding even this metadata would require host
  `hidepid` (out of underwrap's scope). State the guarantee precisely: the
  boundary hides *secret-bearing* `/proc` files and fd re-opens, not world-readable
  process metadata.

Note on `kernel.yama.ptrace_scope=2`: after this story lands it is **no longer
load-bearing** for an isolated target — the uid gate denies READ and ATTACH
alike, before Yama is consulted. It still earns its keep as defense-in-depth: it
covers the `mem`/ptrace/`process_vm_readv` vectors for instances still on the
**unisolated** path, blocks debuggers generally, and blocks the ATTACH half of
the concurrent-same-X case (below). It does **not** protect `environ`/`maps`/
`auxv` for anyone. 2610 rewrites the `hardenTargetHost` claim accordingly; the
guarantee now rests on dumpable=0 (stages) + uid X (target).

## Child tickets (intended order)

1. `2610_task_correct_hardentargethost_and_decisive_vm_test` — fix the false
   claim + stop the VM test from masking the exposure. **Lands the honest
   baseline first** (asserts the hole exists today).
2. `2620_task_dedicated_target_uid_and_capset` — NixOS: per-instance dedicated
   uid/gid + `isolateTargetUid` option; grant `underwrap-ns` **`cap_setuid` and
   `cap_setgid`** (both needed — the gid switch and `setgroups` require
   `CAP_SETGID`). The target uid/gid live **only** in the trusted instance sidecar
   and are read by `underwrap-ns` itself, keyed on its kernel-pinned
   instance-group `egid` — **never relayed through `ExecBundle`/fd 3** (see the
   authority rule below).
3. `2630_task_ns_setresuid_target_uid` — `underwrap-ns` switches the target to
   real uid X (and gid Xgid) before exec. **Protection lands here**
   (`environ`/`maps`/`auxv`/`mem`/ptrace/`process_vm_readv` blocked for the
   caller and all descendants). Also makes the target-facing staging surfaces
   (files-slot tmpfs, libsecret bus tmpfs) owned by X so the uid-X target can use
   them. Targets that need `$HOME` break until 2640 — gated behind the
   off-by-default option.
4. `2640_task_ns_idmapped_home_and_runtime` — idmapped bind mounts of `$HOME` +
   `$XDG_RUNTIME_DIR` (via `open_tree`+`mount_setattr`+`move_mount`) so the uid-X
   target reads/writes the user's files as the user; on-disk ownership stays the
   user. Plus the optional `idmapPaths` for extra user-private paths.
5. `2645_task_harden_exec_boundary_tty_and_fds` — close the **target → caller**
   escalations the uid boundary creates: `setsid()` + seccomp-deny
   `TIOCSTI`/`TIOCLINUX` (terminal injection), and a `close_range` fd sweep +
   `O_CLOEXEC` (Leaky Vessels fd leaks, incl. the new idmap-userns fd). Required,
   not optional.
6. *(optional)* `2650_task_ns_userns_identity_relabel` — process user namespace
   so `getuid()==caller` inside; cosmetic "believes it is robert" fidelity. Not
   required for protection or file access; defer for `$HOME`-env-driven targets.
7. `2660_task_comprehensive_isolation_vm_test` — the decisive end-to-end matrix:
   caller (and descendant-spawned)
   `environ`/`maps`/`auxv`/`mem`/ptrace/`process_vm_readv` all denied; root still
   can read; home rw + on-disk ownership; files-slot + libsecret + writeback
   still work; signal semantics.

## Sequencing notes

- 2610 is independent and lands first (turns the spurious VM-test PASS into a
  real signal).
- 2620 must precede 2630 (the uid must exist + reach `underwrap-ns`).
- 2630 delivers protection; 2640 restores home; 2645 closes the target→caller
  escalations the boundary opens; 2650 (optional) adds cosmetic fidelity. Each is
  independently testable, and the `isolateTargetUid` flag keeps every existing
  wrapper on the current path until its instance opts in. A target is fully
  functional after 2640 even if 2650 is never done.
- **2645 is not optional and must land before any production flip:** an isolated
  instance without it has a real X→caller escalation (TIOCSTI, leaked fds). Treat
  2630+2640+2645 as the minimum coherent isolation set.
- Roll out: enable `isolateTargetUid` for new VM fixture wrappers per ticket;
  flip the real instances (e.g. `aws-sso`) only after 2660 is green.

## Acceptance

Each child ticket carries its own acceptance check and VM-test additions; 2660
is the consolidated proof that the property holds for a target *and* its
descendants, end to end.

## Out of scope / future

- **Per-invocation transient uids** (systemd `DynamicUser`-style: pick a free uid
  from a pool, `flock(LOCK_EX|LOCK_NB)` a `/run/...uid/$UID` lock for the process
  lifetime). The static per-instance uid means two *concurrent* invocations of
  the same instance share uid X and can read each other. Acceptable interim;
  track as a follow-up (`2670_task_per_invocation_target_uid`) if cross-session
  isolation within one instance is required.
- **Supplementary-group access** through the idmapped mounts (only the user's
  primary gid is mapped initially).
- **Real session D-Bus** access for the uid-X target (peer-cred policy rejects a
  foreign uid). underwrap already supplies a private bus for libsecret; targets
  needing the real session bus are not in scope.
