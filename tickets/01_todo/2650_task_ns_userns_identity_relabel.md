# Task: Relabel the target's identity to the caller's uid inside a user namespace (`getuid()==caller`)

Parent: `2600_story_target_uid_isolation`
Depends on: `2640_task_ns_idmapped_home_and_runtime`

## Problem

After 2630/2640 the target is protected (real uid X) and can use the caller's
home (idmapped), but inside it still sees `getuid()==X` and files display as the
dedicated user's name. For full "the target believes it is robert" fidelity — and
to avoid the handful of programs that branch on `getuid()`/displayed user — wrap
the target in a **user namespace** that maps inside-uid = caller (1700) ⇄
host-kuid X. The host still sees uid X (so the protection is unchanged — it comes
from the real uid, not the namespace), but inside, `getuid()==1700` and the world
looks like a normal session.

This must not be confused with protection: the user namespace by itself changes
nothing the host sees — the host kuid is fixed by the 2630 `setresuid` to X. The
userns is purely the inside-relabel layer.

## Design

In `run_child`, after the idmapped mounts (2640) and the `setresuid`/`setresgid`
to X (2630), and before `set_no_new_privs`/`drop_caps`/exec:

1. `unshare(CLONE_NEWUSER)`.
2. Write the process `uid_map` = `1700 X 1` and `gid_map` = `<callergid> Xgid 1`
   (inside-id, outside-id, count). Because the process is **already uid X**,
   mapping its own uid is the unprivileged self-map case — no extra capability
   needed (do **not** rely on this writing arbitrary maps; it's exactly the
   single self-map the kernel allows).
   - Set `/proc/self/setgroups` to `deny` before writing `gid_map` (kernel
     requirement for unprivileged gid_map writes).
3. Proceed to `set_no_new_privs`, `drop_caps`, `make_process_non_dumpable`, exec.

Interactions to get right (reference crun/bwrap ordering):

- The **idmap userns** used for `mount_setattr` in 2640 is a *separate* object
  from this process userns. The idmapped mounts already translate on-disk-1700 →
  kernel-uid X; that is unchanged by entering this process userns (ownership
  checks are kuid-based: target fsuid kuid X == file's mounted kuid X → owner
  access). This userns only changes display/`getuid()` (kuid X →
  `from_kuid(process_userns) = 1700`). Verify the two compose: inside, home files
  show as owned by **1700**, fully accessible.
- Capabilities: entering a userns grants full caps *inside it*, but
  `no_new_privs` + the empty bounding/permitted sets from `drop_caps` (run after
  the userns is set up) must still leave the target with **zero** effective caps
  in the init userns. Re-confirm `caps.rs` behavior after a userns is present
  (the existing `test-caps` fixture must still report all-zero Eff/Prm/Inh/Amb).
- PID namespace: the target is already PID 1 of its PID namespace (existing
  behavior). Owning a new userns + being PID-ns init is fine; keep the fresh
  `/proc` remount (done before, in the original mount ns view).

## Acceptance

- VM fixture `test-iso-id` (isolated instance) prints `getuid()`, `geteuid()`,
  and the `stat` owner-name of a `$HOME` file. Assertions:
  - inside, `getuid() == geteuid() == 1099` (the caller `testuser`), **not** X;
  - the `$HOME` file displays as owned by `testuser`;
  - read **as root** in the VM, the same target's `/proc/<pid>/status` shows real
    uid **X** (host identity unchanged — protection intact).
- The 2630 caller-side protection assertions still hold
  (`environ`/`mem`/`maps`/ptrace EPERM for `testuser`), and the 2640 home rw +
  on-disk-ownership assertions still hold.
- `test-caps` still reports zero capabilities for the target.
- `cargo test -p underwrap-ns`, `cargo fmt --check`, `nix build .#<vm test attr>`
  green.

## Out of scope

- Real session D-Bus (peer-cred sees host uid X regardless of the relabel —
  underwrap's private bus is the supported path).
- Per-invocation uids; supplementary groups.
