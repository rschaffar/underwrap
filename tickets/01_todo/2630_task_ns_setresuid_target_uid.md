# Task: Switch the wrapped target to the dedicated real uid in `underwrap-ns`

Parent: `2600_story_target_uid_isolation`
Depends on: `2620_task_dedicated_target_uid_and_capset`
Blocks: `2640`, `2650`, `2660`

## Problem

This is the ticket that lands the actual protection. When `ExecBundle` carries a
`target_uid`/`target_gid`, the child must adopt that **real** uid before exec so
that same-UID (uid-1700) callers fail the `__ptrace_may_access` gate
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
setgroups([target_gid])              // drop the caller's supplementary groups
setresgid(gid, gid, gid)             // needs CAP_SETGID? no — covered by CAP_SETUID for uid; gid needs CAP_SETGID
setresuid(uid, uid, uid)             // X for real+eff+saved; needs CAP_SETUID
set_no_new_privs()                   // after the id switch
drop_caps()                          // now drop cap_setuid/cap_sys_admin/etc.
make_process_non_dumpable()          // belt-and-suspenders (exec also makes it non-dumpable since this is now a normal same-uid exec INSIDE uid X... see note)
... build env ...
cmd.exec()
```

Implementation notes:

- Add `cap_setgid` to the `underwrap-ns` wrapper caps in 2620's module change if
  not already present (the gid switch needs it); update that ticket's capability
  string if so (`cap_setuid,cap_setgid`). Keep both dropped before exec.
- Order matters: do the **gid** switch before the **uid** switch (once euid≠0 and
  you've dropped the uid you can't change gids). Drop supplementary groups first
  with `setgroups`.
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
  target command setup; after this change those helpers also run as X. Verify
  `setup_libsecret` still works post-switch (bus dir perms, state path). They
  serving the target over the private bus as the same uid X is correct and also
  hides them from the caller.
- `build_target_env` (`env.rs`): keep `HOME` and `XDG_RUNTIME_DIR` pointing at
  the caller's real values (the idmapped mounts in 2640 make them accessible).
  No change needed here yet; note that until 2640 lands, a target that touches
  `$HOME` will get EACCES — that's expected and gated behind `isolateTargetUid`.

## Acceptance

- With `isolateTargetUid = true`, the `test-iso-env` fixture target runs as uid
  X (assert via `/proc/<pid>/status` `Uid:` line read **as root** in the VM, or
  by having a fixture print `id -u`).
- **The protection (VM test, `test.py`):** as `testuser` (uid 1099), reading the
  isolated target's `/proc/<pid>/environ` returns **EPERM**; `mem` and `maps`
  also EPERM; `ptrace(PTRACE_ATTACH)` EPERM. The unprotected `test-sleep`
  baseline still shows `environ` readable (proving the difference is the uid
  switch, not the harness). This flips the 2610 `isolateTargetUid=true` branch
  from expected-readable to denied.
- **Descendant coverage:** add a fixture that `fork/exec`s a child `sleep` and
  assert the *child's* `/proc/<pid>/environ` is also EPERM for `testuser`
  (proves subtree inheritance).
- A failed `setresuid`/`setresgid` aborts the run (the target never execs as the
  caller) — unit-test the error path where possible.
- Parent (`underwrap-ns` supervisor) still completes writeback/cleanup (it stays
  at the caller uid) — existing writeback VM scenarios still pass.
- `cargo test -p underwrap-ns`, `cargo fmt --check`, `nix build .#<vm test attr>`
  green.

## Out of scope

- Home/socket access for the uid-X target (2640) and the `getuid()==caller`
  relabel (2650). Until 2640, isolated targets can't read `$HOME` — only enable
  `isolateTargetUid` for fixtures that don't need it (`env`, `sleep`).
