# Task: Per-target sandbox users + root-owned target→uid table + capset

Parent: `2600_story_target_uid_isolation`
Blocks: `2630_task_ns_setresuid_target_uid`

## Problem

To make a wrapped target's secret unreadable by same-UID callers, each isolated
target must run under its **own dedicated uid X** (story 2600). That uid must
(a) exist with a passwd entry so `getpwuid(X)` works, (b) be otherwise unused, and
(c) be communicated to `underwrap-ns` via a path the *unprivileged* stages cannot
forge — because the uid is the one value whose attacker-control means privilege
**escalation** (`X = 0` → a root-owned target). `underwrap-ns` also needs
`CAP_SETUID`/`CAP_SETGID` to perform the switch (it has `cap_sys_admin` +
`cap_setpcap` today — `nix/modules/underwrap.nix` — but not setuid/setgid).

This ticket is the **wiring** (users, table, caps). The actual `setresuid` lands
in 2630.

## Fix — all in `nix/modules/underwrap.nix`

### Per-wrapper opt-in

- Add a per-wrapper `isolate` option (bool, `mkDefault false`). When true, the
  wrapper's target runs under a dedicated uid (the protection lands in 2630).
  Off-by-default keeps every existing wrapper on the current path until it opts
  in. (A convenience instance/global default may set it for many wrappers at once;
  the unit of isolation is the **target binary path** — see below.)

### Per-target sandbox users

- For each **distinct target binary path** used by an isolated wrapper (across all
  instances), create one dedicated system user/group, e.g.
  `underwrap-target-<n>` (`isSystemUser = true`, matching group). The exact name
  is implementation-defined; what matters is a **stable, unique uid per target
  path**.
  - Passwd entry (kills the NULL-`getpwuid` path), `/sbin/nologin`, no home.
  - **Security property — the dedicated user must NOT be a member of any instance
    group `underwrap-<instance>`** (nor any other privileged group). Otherwise the
    uid-X target could satisfy the wrapper-socket egid gate and re-invoke wrappers
    to pull further secrets. Assert in the eval/VM test that
    `id underwrap-target-<n>` shows only its own primary group.
  - Wrappers that share the same target binary path share its uid (same tool, same
    trust domain — story 2600). Document this; finer granularity is `2670`.
  - Allow an operator override of the uid/gid per target if needed.

### The root-owned target→uid table (the authority)

- Write a **root-owned** table (e.g. `/etc/underwrap/target-uids.json`, mode
  `0644`, root:root — the content is non-secret: absolute paths + uid/gid numbers)
  mapping each isolated target's **absolute binary path → `{ uid, gid }`**. The
  NixOS module is the sole writer; `underwrap-ns` reads it (2630).
  - This file is the **single source of truth** for which uid a target runs as.
    It is read directly by the privileged stage and never travels on the wire,
    never comes from the wrapper, the daemon response, or the `ExecBundle`. Because
    it is root-owned, a compromised/buggy `underwrap-exec` cannot influence it.
  - Document in `docs/security.md` as a design invariant alongside *"the daemon —
    not the wrapper — chooses the target binary"*: **the target uid is chosen by
    trusted root config keyed on the target binary path, read directly by the
    privileged stage; it is never relayed.**
  - `ExecBundle` carries **no** target uid (no new wire field). `underwrap-exec`
    is unchanged for this feature.

### Capabilities

**Decision to record here: `cap_setuid` vs. a per-target setuid-X launcher.**
The uid switch needs an unrestricted `CAP_SETUID` (become *any* uid, incl. 0) on
`underwrap-ns`, OR a per-target setuid-X launcher (bounded to X, no capability).
The rationale for why `cap_setuid` is acceptable — `underwrap-ns` already holds
`CAP_SYS_ADMIN` in the caller's process under a mandatory drop-before-exec
discipline; the root-owned table + a floor check (not a cap limit) defend against
`X=0` — and the bounded setuid-X alternative (with its `NO_NEW_PRIVS` ordering
wrinkle) are written up in **2600 "Why `cap_setuid` is acceptable here (and the
bounded alternative)"**. **Make the choice explicitly and record it here before
implementing 2630.** The default below is `cap_setuid`; switch to setuid-X if we
decide to shrink `underwrap-ns`'s privilege rather than grow it.

- Grant `underwrap-ns` **`cap_setuid` and `cap_setgid`** **only when at least one
  wrapper sets `isolate = true`** (keep the cap surface minimal otherwise):
  `capabilities = "cap_sys_admin,cap_setpcap,cap_setuid,cap_setgid+ep"`. Both are
  needed transiently and dropped before the target exec (2630): `setresuid(X,…)`
  needs `CAP_SETUID` (`kernel/sys.c`); `setresgid(Xgid,…)` + `setgroups` need
  `CAP_SETGID` (`kernel/sys.c`, `kernel/groups.c`); `cap_setgid` is also required
  for the idmap gid_map write in 2640.
  - The NixOS `security.wrappers` setcap-wrapper raises file caps into the ambient
    set and only skips `cap_setpcap` (nixpkgs `wrapper.c`), so `cap_setuid`/
    `cap_setgid` land effective exactly like `cap_sys_admin`. (This is why
    `drop_caps` must clear the ambient set — it already does — so the caps don't
    leak across the target exec.)
  - **No egid relocation, no privilege-drop surgery.** `underwrap-ns` runs as the
    caller's uid with these file caps, which is sufficient to `setresuid` to X —
    the authority for *which* uid comes from the root-owned table, not from any
    credential `underwrap-ns` holds. (This is the simplification over the abandoned
    egid-badge design — see 2600 "What we abandoned and why.")

## Acceptance

- New per-wrapper `isolate` option exists and is documented; with `isolate = true`
  the eval produces a dedicated `underwrap-target-<n>` system user (passwd entry)
  for that target path, a member of **only** its own group.
- The root-owned `/etc/underwrap/target-uids.*` table is generated, lists the
  isolated targets' absolute paths → uid/gid, and is `root:root` and not
  world-writable.
- `getcap /run/wrappers/bin/underwrap-ns` includes `cap_setuid` **and**
  `cap_setgid` **iff** some wrapper enables isolation; otherwise unchanged.
- `ExecBundle` carries **no** target-uid field (a `underwrap-protocol` round-trip
  test confirms this); `underwrap-exec` is unchanged for this feature.
- VM node (`nix/tests/node.nix`): add an isolated fixture wrapper `test-iso-env`
  (target = `${pkgs.coreutils}/bin/env`, `env = true`, `isolate = true`, resource
  `test/iso-tok`) plus a matching grant + secret in `test.py`
  (`ISO_SECRET=iso_secret_val`). Keep an **unisolated** `test-sleep` for the
  2610/2660 contrast. At this ticket the target still execs as the caller (2630
  flips it); assert only that it runs and that the user/table/caps were produced.
- `cargo test -p underwrap-protocol -p underwrap-exec`, `cargo fmt --check`,
  `nix build .#<vm test attr>` green.

## Out of scope

- The `setresuid` itself and the `environ`-denied assertions (2630).
- Per-wrapper / per-invocation uids finer than per-target-path (future `2670`).
