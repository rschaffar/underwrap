# Task: Switch the wrapped target to the dedicated real uid in `underwrap-ns`

Parent: `2600_story_target_uid_isolation`
Depends on: `2620_task_dedicated_target_uid_and_capset`
Blocks: `2640`, `2650`, `2660`

## Problem

This is the ticket that lands the actual protection. When the instance's trusted
sidecar declares `target_uid`/`target_gid` — read by `underwrap-ns` itself,
keyed on its own `getegid()` (the kernel-pinned instance group), **never relayed
through `ExecBundle`** (see 2620) — the child must adopt that **real** uid before
exec so that same-UID (uid-1700) callers fail the `__ptrace_may_access` gate
(`uid_eq(caller, target.uid) && ...suid && ...euid` — all three must match) and
therefore cannot read the target's `environ`/`mem`/`maps`/`auxv`, ptrace it, or
`process_vm_readv` it. The whole subtree is covered because children inherit the
real uid.

## Fix

All changes in `crates/underwrap-ns/src/main.rs` `run_child` (and a small helper
in `caps.rs`). The parent (`run_parent`) **must stay at the caller uid + caps**
for mount cleanup and writeback — do not change it.

Current `run_child` order (lines ~199-250):
```
remount_proc()
[setup_mount_namespace() if libsecret]
set_no_new_privs()
drop_caps()
disable_core_dumps()
make_process_non_dumpable()
... build env ...
cmd.exec()
```

New order when `target_uid`/`target_gid` are present:
```
remount_proc()
[setup_mount_namespace() if libsecret]
[2640 will insert idmapped HOME/runtime mounts HERE — still holding caps]
disable_core_dumps()
setgroups([target_gid])              // drop caller's supplementary groups — needs CAP_SETGID
setresgid(gid, gid, gid)             // Xgid for real+eff+saved — needs CAP_SETGID
setresuid(uid, uid, uid)             // X for real+eff+saved   — needs CAP_SETUID
set_no_new_privs()                   // after the id switch
drop_caps()                          // now drop cap_setuid/cap_setgid/cap_sys_admin/etc. + ambient
make_process_non_dumpable()          // belt-and-suspenders only; exec resets dumpable (see note)
... build env ...
cmd.exec()
```

Implementation notes:

- `cap_setgid` **and** `cap_setuid` are granted by 2620 (the gid switch and
  `setgroups` both need `CAP_SETGID` — `kernel/sys.c:846`, `kernel/groups.c:189`;
  the uid switch needs `CAP_SETUID` — `kernel/sys.c:745-747`). Both are dropped
  before exec. The NixOS wrapper delivers both effectively (only `cap_setpcap` is
  stripped — see 2620).
- Order matters: drop supplementary groups first with `setgroups([Xgid])`, then
  the **gid** switch, then the **uid** switch (once euid≠0 and you've dropped the
  uid you can no longer change gids).
- **Source the target uid/gid from trusted config, anchored on `getegid()`
  (see 2620) — it is not relayed.** `underwrap-ns` selects the sidecar whose
  recorded instance gid matches its own `getegid()` and reads `target_uid`/
  `target_gid` from there; the value never arrives on fd 3/env/argv. This makes a
  relayed value irrelevant — the only id `underwrap-ns` will switch to is the one
  trusted `/etc` assigns to the kernel-authenticated instance. Keep a
  defense-in-depth sanity bound anyway: reject `0` / below the system-uid floor /
  equal to the caller's ruid-rgid; `bail!` before any namespace/exec work.
  Unit-test the rejection paths (incl. a sidecar that somehow specifies `0`).
- Use `setresuid`/`setresgid` (not `setuid`) and check return values; on failure
  `bail!` — never exec a target that failed to drop to X (that would run it as
  the caller with secrets in a readable env).
- `set_no_new_privs` and `drop_caps` move to **after** the id switch (we need
  `CAP_SETUID`/`CAP_SETGID` to perform it). Confirm the bounding-set/`capset`
  logic in `caps.rs` still zeroes everything post-switch.
- **Dumpable note:** with real=eff=saved=X (all equal), the target exec is a
  *normal* same-uid exec, so `begin_new_exec` resets dumpable to
  `SUID_DUMP_USER`. That's fine — the **uid gate** (caller 1700 ≠ target X) is
  the protection, not dumpable. `make_process_non_dumpable()` right before exec
  is retained only as defense-in-depth for the brief pre-exec window and is
  understood to be reset by the exec. Add a comment saying so (kills the old
  misconception).
- Helper env for libsecret (`dbus-daemon`, `secretsd`) is launched as part of the
  target command setup (`setup_libsecret`, after the id switch), so those helpers
  now also run as X — correct, and it additionally hides them from the caller.

- **Target-facing staging surfaces must be made accessible to uid X (blocker for
  files-slot and libsecret).** Today both are created owned by the caller (1700)
  *before* the switch and mounted `mode=0700`, so the uid-X target cannot use
  them:
  - the files-slot staging tmpfs + bind mounts (`mounts.rs:setup_directory_staging_tmpfs`,
    `size=8m,mode=0700`; materialized in the **parent** at `main.rs:75,142`), and
  - the libsecret bus tmpfs (`mounts.rs:setup_mount_namespace`,
    `/tmp/.underwrap-ns-bus`, `mode=0700`), where `dbus-daemon` (X) creates the
    socket and `secretsd` (X) writes `libsecret-state.jcs`.

  These have a **cross-uid** access pattern: the uid-X target subtree reads/writes
  them, **and** the 1700 supervisor reads them at writeback (it walks the staging
  tmpfs and reads `libsecret-state.jcs`). They cannot simply be chowned to X
  (`underwrap-ns` has no `CAP_CHOWN`). **World-readable perms (`mode=0777`) are
  *not* sufficient** for the files-slot: a uid-X target commonly *replaces*
  projected files (atomic temp+rename) or creates new ones, which land owned by X
  with the target's umask (often `0600`) — and the 1700 supervisor (no
  `CAP_DAC_OVERRIDE`) then cannot read them at writeback. Use one of:
  - **(preferred) supervisor borrows fsuid X via `CAP_SETUID`.** Mount the
    staging/bus tmpfs `uid=X,gid=Xgid,mode=0700` (owned by X — confidential even
    if the private ns were ever escaped). The supervisor — which holds
    `CAP_SETUID`/`CAP_SETGID` — does `setfsuid(X)`/`setfsgid(Xgid)` around
    materialization (pre-fork, caps held) and around the writeback reads, then
    restores `setfsuid(1700)`/`setfsgid` immediately. `setfsuid` to an arbitrary
    id is permitted with `CAP_SETUID`. **This requires keeping the supervisor's
    caps until after writeback**: today `run_parent` drops caps *early* at
    `main.rs:298` for the `!has_files` path (which includes libsecret-only
    wrappers), *before* `wait_for_child`/writeback — change it so an isolated
    instance retains `CAP_SETUID`/`CAP_SETGID` through the writeback and drops
    uniformly afterward.
  - **(alternative) idmap the target's view, supervisor reads the non-idmapped
    view.** Reuse 2640's idmap machinery: present the staging/bus to the uid-X
    target through an idmapped clone (1700⇄X) while the supervisor walks the
    original (non-idmapped) mount, where target-created files are on-disk-1700 =
    owner. No cap retention needed at writeback. Heavier, but uniform with home.

  Either way, confidentiality rests on the **private mount namespace** (the caller
  cannot see these mounts at all; concurrent invocations get their own mount
  namespace) plus dbus peer-cred for the bus. Add a VM assertion that the caller
  cannot see/enter these paths from the host namespace.
- `build_target_env` (`env.rs`): keep `HOME` and `XDG_RUNTIME_DIR` pointing at
  the caller's real values (the idmapped mounts in 2640 make them accessible).
  No change needed here yet; note that until 2640 lands, a target that touches
  `$HOME` will get EACCES — that's expected and gated behind `isolateTargetUid`.

## Acceptance

- With `isolateTargetUid = true`, the `test-iso-env` fixture target runs as uid
  X (assert via `/proc/<pid>/status` `Uid:` line read **as root** in the VM, or
  by having a fixture print `id -u`).
- **The protection (VM test, `test.py`):** as `testuser` (uid 1099), reading the
  isolated target's `/proc/<pid>/environ` returns **EPERM**, and so do `maps`,
  `auxv`, `mem`, and `ptrace(PTRACE_ATTACH)` — all via the uid gate. The
  unprotected `test-sleep` baseline still shows `environ` **and** `maps`/`auxv`
  readable (proving the difference is the uid switch, not the harness, and that
  the READ-mode files were never ptrace_scope-protected). This flips the 2610
  `isolateTargetUid=true` branch from expected-readable to denied.
- **Descendant coverage:** add a fixture that `fork/exec`s a child `sleep` and
  assert the *child's* `/proc/<pid>/environ` is also EPERM for `testuser`
  (proves subtree inheritance).
- A failed `setresuid`/`setresgid` aborts the run (the target never execs as the
  caller) — unit-test the error path where possible.
- **Egid-anchored target id (hard):** `underwrap-ns` obtains `target_uid`/
  `target_gid` from the trusted sidecar selected by its own `getegid()`, **not**
  from `ExecBundle` (which carries no such field). It additionally rejects a
  sidecar-declared `0` / sub-floor / caller-equal id and `bail!`s before any exec.
  Unit-test the rejection paths. A VM assertion confirms the isolated target's
  real uid is exactly the configured `underwrap-target-<instance>` uid (never `0`,
  never the caller), and that a wrapper run with a deliberately bogus fd-3 bundle
  cannot change which uid the target lands on (the bundle has no say).
- Parent (`underwrap-ns` supervisor) still completes writeback/cleanup (it stays
  at the caller uid) — existing writeback VM scenarios still pass.
- **Files-slot and libsecret work under `isolateTargetUid = true`** (neither
  needs `$HOME`, so both must function before 2640): the uid-X target reads its
  projected files-slot content and writes back; a libsecret store/lookup over the
  private bus succeeds with `dbus-daemon`/`secretsd` running as X; the 1700
  supervisor still reads the staging files / `libsecret-state.jcs` at writeback.
  Plus the VM assertion that the caller cannot see these staging paths from the
  host namespace.
- `cargo test -p underwrap-ns`, `cargo fmt --check`, `nix build .#<vm test attr>`
  green.

## Out of scope

- Home/socket access for the uid-X target (2640) and the `getuid()==caller`
  relabel (2650). Until 2640, isolated targets can't read `$HOME` — only enable
  `isolateTargetUid` for fixtures that don't need it (`env`, `sleep`).
