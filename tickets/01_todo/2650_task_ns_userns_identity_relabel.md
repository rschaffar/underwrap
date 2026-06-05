# Task: Relabel the target's identity to the caller's uid inside a user namespace (`getuid()==caller`)

Parent: `2600_story_target_uid_isolation`
Depends on: `2640_task_ns_idmapped_home_and_runtime`

> **Optional / opt-in.** This ticket is **not** required for protection or for
> file access — both are fully delivered by 2630 (uid X) + 2640 (idmapped
> mounts). It only changes `getuid()`/displayed-name cosmetics. Because the
> idmapped home already presents files to the target as owned by X *and*
> `getuid()==X`, `st_uid == getuid()` ownership checks (e.g. ssh's `~/.ssh`
> check) already pass without it. Defer for `$HOME`-env-driven targets (e.g.
> `aws-sso`); enable it only for targets that branch on `getuid()` or resolve
> `$HOME` via `getpwuid(getuid())->pw_dir` rather than the `HOME` env var.
>
> **Cost — this is a real security trade-off, not free fidelity.** A user
> namespace is the dominant *enabling* primitive for the 2020–2025 local-privesc
> CVE class (CVE-2022-0185 fs-context, CVE-2023-0386 overlayfs, CVE-2023-32233
> nf_tables, …): merely *having* one reachable arms those bugs, because the
> target becomes "root in its own userns" and unlocks kernel code paths an
> unprivileged process can't otherwise reach. We hand that surface to a
> *deliberately sandboxed, possibly-malicious* target. Weigh that against a
> cosmetic `getuid()`. Default off, per-wrapper opt-in.

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

> **Placement:** this is the `[2650]` step in the **canonical ordering in 2630**
> ("the SPINE") — immediately after `setresuid` to X and before
> `set_no_new_privs`/`drop_caps`/exec.

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
- Capabilities: `unshare(CLONE_NEWUSER)` grants the process a full capability set
  *in the new userns* (and those caps mean nothing against init-userns-owned
  resources — the target's kuid stays X, so it has no authority over real files
  or other-uid processes). `drop_caps` runs **after** the userns is set up and
  must still leave the target with **zero** effective caps. Note `drop_caps`
  zeroes Eff/Prm/Inh/Amb via `capset`; the **bounding** set is a separate matter
  — today `PR_CAPBSET_DROP` EPERMs pre-userns (no `cap_setpcap`), but *after* the
  userns unshare the process holds `cap_setpcap` in the new ns, so the bounding
  set can now be fully dropped too. Either way the `test-caps` fixture asserts
  zero **Eff/Prm/Inh/Amb** (not Bnd), and `no_new_privs` prevents regaining caps
  across exec. Re-confirm `caps.rs` behavior with a userns present.
- The userns does **not** weaken the protection: the host kuid is fixed at X by
  the 2630 `setresuid` and is unchanged by entering a userns, so a uid-1700
  caller still mismatches the uid gate; and the caller has no `CAP_SYS_PTRACE` in
  the target's userns (`ns_capable(tcred->user_ns, …)` is false for an
  unprivileged ancestor — `kernel/ptrace.c:337`), so `environ`/`mem` stay EPERM.
- PID namespace: the target is already PID 1 of its PID namespace (existing
  behavior). Owning a new userns + being PID-ns init is fine; keep the fresh
  `/proc` remount (done before, in the original mount ns view).
- **The host may forbid the target's `unshare(CLONE_NEWUSER)`.** Ubuntu 24.04+
  ships `kernel.apparmor_restrict_unprivileged_userns=1`; Debian/others expose
  `kernel.unprivileged_userns_clone=0`. After 2630 the target is an *unprivileged*
  uid X, so its own `unshare(CLONE_NEWUSER)` can `EPERM` on exactly the hardened
  hosts users deploy on purpose. Two consequences: (a) **fail closed** — if the
  userns can't be created, abort the run (no insecure fallback, per project rule),
  do **not** silently drop to the 2640-only path; and (b) prefer creating the
  userns from the **cap-holding stage** (which is not subject to the unprivileged
  restriction) before the final `setresuid`, rather than from the dropped target —
  mirrors 2640's "privileged stage builds the artifact" rule. Because 2650 is
  opt-in, an instance that needs it on a restricting host must be a conscious
  choice.
- **Single-extent map only.** Use exactly one uid extent (`1700 X 1`) and one gid
  extent. This both keeps in-userns authority near-zero (only X is mapped;
  everything else is overflow/`nobody`, immune to the target's in-ns "root") and
  sidesteps **CVE-2018-18955** (the kernel mis-translated nested maps with >5
  extents, making ownership/capability checks return wrong answers). Never nest
  this userns inside another large-map userns.
- **Map consistency / `EOVERFLOW`.** The process-userns map and the 2640 mount
  idmap must agree on X (the target's fsuid must be the mapped id). A mismatch
  makes idmapped-mount writes fail `EOVERFLOW` rather than silently mis-own —
  treat that as a hard configuration error, not a fallback.

## Acceptance

- VM fixture `test-iso-id` (isolated) prints `getuid()`, `geteuid()`,
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
