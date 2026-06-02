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
source (v6.12, unchanged on 6.18):

- `/proc/<pid>/environ` is opened with `PTRACE_MODE_READ` (`environ_open` →
  `proc_mem_open` adds `FSCREDS`). Yama's `yama_ptrace_access_check` only acts on
  `PTRACE_MODE_ATTACH`, so **ptrace_scope never gates `environ`**. (It *does* gate
  `/proc/<pid>/mem` and `maps`, which use `PTRACE_MODE_ATTACH` — so those were
  genuinely protected, masking the hole.)
- The only gate left for `environ` is the target's **dumpable** flag, and a
  normal same-UID `execve` of a world-readable binary resets dumpable to
  `SUID_DUMP_USER=1` (`begin_new_exec`). So `underwrap-ns`'s
  `make_process_non_dumpable()` immediately before `cmd.exec()` is a **no-op for
  the target** — the exec wipes it.

Net effect: every exec target (`aws-sso`, the shell it spawns, `dbus-daemon`) is
readable via `/proc/<pid>/environ` by any other uid-1700 process. Only the
non-exec'ing wrapper stages (`underwrap-exec`, the `underwrap-ns` supervisor,
`underwrap-secretsd`) are protected.

## The fix — a real UID boundary

The `__ptrace_may_access` gate (which guards `environ`, `mem`, `maps`, `auxv`,
`ptrace`, `process_vm_readv`) requires the caller's uid to equal **all three** of
the target's real, effective, and saved uid. The only robust way to make a
target unreadable by uid-1700 processes is to give it a **different real uid X**.
That protects the whole subtree (children inherit the real uid), and as a bonus
makes every exec a secure-exec → non-dumpable.

To keep the target usable as the human user without the Option-A/-B sharp edges
(bash drops euid when euid≠ruid; `getuid()` confusion; broken home/socket
access), we layer:

- a **dedicated uid X** per instance (the protection),
- **idmapped bind mounts** of the user's `$HOME` and `$XDG_RUNTIME_DIR` so the
  uid-X target reads/writes the real user's files as that user, writes landing
  on disk owned by the real user (the convenience),
- a **user namespace** that relabels X ↔ the caller's uid inside, so
  `getuid()==1700` and the world looks like a normal session (full fidelity —
  "the target believes it is robert").

This is the rootless-container model (`podman --userns=keep-id` + `:idmap`),
implemented natively in `underwrap-ns` rather than by adopting a container
runtime — the broker/secretsd/fd-channel architecture is underwrap's and stays
underwrap's; only the sandboxed-exec layer grows.

## What this does NOT defend against (document, don't pretend)

- **root.** A caller who can `sudo` gets `CAP_SYS_PTRACE` and bypasses the uid
  gate. No same-host mechanism hides memory from root. This protects against
  *unprivileged* same-uid code (compromised/malicious processes running as the
  user), which is the realistic threat.
- **Secrets the target deliberately persists** into the user's home (e.g.
  `aws-sso` caching creds under `~/.aws`) are the user's files on disk and remain
  readable by the user. Use the libsecret/secretsd path (Option D) for secrets
  that must never hit disk.

## Child tickets (intended order)

1. `2610_task_correct_hardentargethost_and_decisive_vm_test` — fix the false
   claim + stop the VM test from masking the exposure. **Lands the honest
   baseline first** (asserts the hole exists today).
2. `2620_task_dedicated_target_uid_and_capset` — NixOS: per-instance dedicated
   uid/gid + `isolateTargetUid` option; grant `underwrap-ns` `cap_setuid`; thread
   the target uid/gid to `underwrap-ns` via the instance sidecar + `ExecBundle`.
3. `2630_task_ns_setresuid_target_uid` — `underwrap-ns` switches the target to
   real uid X before exec. **Protection lands here** (env/mem/ptrace blocked for
   the caller and all descendants). Targets that need `$HOME` break until 2640 —
   gated behind the off-by-default option.
4. `2640_task_ns_idmapped_home_and_runtime` — idmapped bind mounts of `$HOME` +
   `$XDG_RUNTIME_DIR` so the uid-X target reads/writes the user's files as the
   user; on-disk ownership stays the user.
5. `2650_task_ns_userns_identity_relabel` — process user namespace so
   `getuid()==caller` inside; full "believes it is robert" fidelity.
6. `2660_task_comprehensive_isolation_vm_test` — the decisive end-to-end matrix:
   caller (and descendant-spawned) `environ`/`mem`/`maps`/ptrace/`process_vm_readv`
   all denied; root still can read; home rw + on-disk ownership; signal
   semantics; libsecret + writeback still work.

## Sequencing notes

- 2610 is independent and lands first (turns the spurious VM-test PASS into a
  real signal).
- 2620 must precede 2630 (the uid must exist + reach `underwrap-ns`).
- 2630 delivers protection; 2640 restores home; 2650 adds fidelity. Each is
  independently testable, and the `isolateTargetUid` flag keeps every existing
  wrapper on the current path until its instance opts in.
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
