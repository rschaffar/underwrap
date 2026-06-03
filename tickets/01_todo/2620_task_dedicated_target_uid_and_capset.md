# Task: Allocate a dedicated target uid/gid per instance and plumb it to `underwrap-ns`

Parent: `2600_story_target_uid_isolation`
Blocks: `2630_task_ns_setresuid_target_uid`

## Problem

To make a wrapped target unreadable by same-UID callers, it must run under a
**different real uid X** (story 2600). That uid must (a) exist with a passwd
entry so `getpwuid(X)` doesn't return NULL, (b) be otherwise unused, and (c) be
communicated to `underwrap-ns`, which currently has no idea such a uid exists.
`underwrap-ns` also needs `CAP_SETUID` to switch to it (it has `cap_sys_admin`
and `cap_setpcap` today — `nix/modules/underwrap.nix:583` — but not setuid).

## Fix (this ticket = wiring only; the actual setresuid is 2630)

### Module option + uid allocation (`nix/modules/underwrap.nix`)

- Add a per-instance option `isolateTargetUid` (bool, `mkDefault false`). When
  true, the instance runs its wrapped targets under a dedicated uid/gid.
- Add per-instance `targetUid` / `targetGid` options. Default: auto-create a
  system user/group `underwrap-target-<instance>` (NixOS `isSystemUser = true`,
  matching group) and use its uid/gid. Allow an operator override.
  - The dedicated user gets a passwd entry (kills the NULL-`getpwuid` failure
    path), `/sbin/nologin` shell, no home, no extra groups.
- Grant `underwrap-ns` **`cap_setuid` and `cap_setgid`** **only when at least one
  instance sets `isolateTargetUid = true`** (keep the cap surface minimal
  otherwise): `capabilities = "cap_sys_admin,cap_setpcap,cap_setuid,cap_setgid+ep"`.
  Both are needed transiently and dropped before the target exec (2630):
  `setresuid(X,X,X)` needs `CAP_SETUID` (`kernel/sys.c:745-747`), and
  `setresgid(Xgid,…)` **plus** `setgroups([Xgid])` need `CAP_SETGID`
  (`kernel/sys.c:846`, `kernel/groups.c:189`). `cap_setgid` is also required to
  write the idmap userns gid_map in 2640. Document in `docs/security.md` (why
  needed, that both are dropped pre-exec, **and the egid-anchored trust path
  below** — add it as a design invariant alongside "the daemon, not the wrapper,
  chooses the target binary": *the target uid/gid are chosen by trusted config
  located via the kernel-pinned instance-group egid, and never travel on the wire
  or come from the wrapper*).
  - Verified the NixOS `security.wrappers` setcap-wrapper actually delivers these
    to the effective set: it raises every file cap into the ambient set and only
    special-cases/skips `cap_setpcap` (nixpkgs `wrapper.c:131-136`), so
    `cap_setuid`/`cap_setgid` land effective exactly like the existing
    `cap_sys_admin`. (This is also why `drop_caps` must clear the ambient set —
    it already does — so the caps do not leak across the target exec.)

### Sidecar plumbing — the target uid is **never relayed**, never on the wire

The target uid/gid are a privileged decision, so they must come from an authority
the *unprivileged* `underwrap-exec` cannot influence — **not** relayed through the
`ExecBundle`. The authority is the kernel-pinned instance identity + trusted
`/etc`:

- **`underwrap-ns` runs with `egid` = the instance group** (`underwrap-exec` is
  setgid `underwrap-<instance>` — `nix/modules/underwrap.nix:131` — and
  `underwrap-ns` is a file-cap wrapper, not setgid, so the egid propagates across
  the exec; `underwrap-exec` has no `cap_setgid` to forge a different one). This
  is the *same* kernel identity the daemon already authenticates via
  `SO_PEERPIDFD`.
- The instance sidecar `/etc/underwrap/instances/<i>.yaml` is root-written (NixOS
  module is the sole writer); `underwrap-exec` can read but not forge it.

So:

- **Sidecar (`nix/modules/underwrap.nix`)** — when `isolateTargetUid` is set,
  write `target_uid` / `target_gid` into the per-instance sidecar, **plus the
  instance group's gid** so the file is self-identifying.
- **`underwrap-ns` reads the sidecar itself, keyed by its own `getegid()`** — it
  selects the sidecar whose recorded instance gid `== getegid()` (e.g. scan
  `/etc/underwrap/instances/*.yaml`, or resolve `getgrgid(getegid())` →
  `underwrap-<i>`), and takes `target_uid`/`target_gid` from there. The parser
  (`parse_instance_config_text`) is shared with/duplicated from `underwrap-exec`
  (move it to `underwrap-protocol` if cleaner). This is the authoritative — and
  only — source of the target id.
- **Do NOT add `target_uid`/`target_gid` to `ExecBundle`.** The id never travels
  on fd 3, env, or argv. `underwrap-exec` is unchanged for this feature (it does
  not read, validate, or relay the target id — it doesn't need to). If
  `parse_instance_config_text`'s struct gains the fields, `underwrap-exec` simply
  ignores them.

**Why anchored, not relayed (this is the design rule, mirroring "the daemon — not
the wrapper — chooses the target binary").** If `underwrap-ns` trusted a relayed
uid, a compromised/buggy `underwrap-exec` could hand it `target_uid=0`; with
`cap_setuid`, `setresuid(0,0,0)` succeeds unconditionally (`kernel/sys.c:745-747`
— no uid floor) and a uid-0 target *owns* root-owned files (owner-write on
`/etc/passwd`) even after caps are dropped → full root escalation, defeating the
whole reason the exec→ns split exists. By deriving the id from `getegid()` →
trusted `/etc`, the only uid `underwrap-ns` will ever switch to is the one trusted
config assigns to the kernel-authenticated instance — root / another service /
another instance's uid are structurally impossible, not merely blocklisted. The
actual `setresuid` + a defense-in-depth sanity bound (reject `0`/sub-floor) land
in 2630.

No behavior change in `underwrap-exec`; current instances and the `ExecBundle`
round-trip tests are unaffected (no new wire field). Scope split: **this ticket**
writes the sidecar fields (+ instance gid), grants the caps, and creates the
dedicated user; the egid-keyed **read + `setresuid`** land in **2630** (until
then `underwrap-ns` ignores the new sidecar fields and the fixture still execs as
the caller).

## Acceptance

- New per-instance options `isolateTargetUid`, `targetUid`, `targetGid` exist
  and are documented; with `isolateTargetUid = true` and defaults, the eval
  produces a system user `underwrap-target-<instance>` with a passwd entry.
- `getcap /run/wrappers/bin/underwrap-ns` includes `cap_setuid` **and**
  `cap_setgid` **iff** some instance enables isolation; otherwise unchanged.
- `ExecBundle` does **not** carry `target_uid`/`target_gid` (no new wire field);
  `underwrap-exec` is unchanged for this feature. The per-instance sidecar carries
  `target_uid`/`target_gid` plus the instance group gid, and `underwrap-ns`
  selects the sidecar matching its own `getegid()`. A `underwrap-protocol`
  round-trip test confirms `ExecBundle` has no target-id field.
- VM node (`nix/tests/node.nix`): add a new local fixture wrapper
  `test-iso-env` (target = `${pkgs.coreutils}/bin/env`, `env = true`,
  resource `test/iso-tok`) on an instance configuration with
  `isolateTargetUid = true`, plus the matching grant + secret in `test.py`
  (`DUMPABLE`-style marker, e.g. `ISO_SECRET=iso_secret_val`). At this ticket the
  wrapper still execs as the caller (2630 flips it); assert only that it runs and
  prints its env (no isolation assertions yet).
- `cargo test -p underwrap-protocol -p underwrap-exec`, `cargo fmt --check`,
  `nix build .#<vm test attr>` green.

## Out of scope

- The `setresuid` itself and the `environ`-denied assertions (2630).
- Per-invocation transient uids (future `2670`).
