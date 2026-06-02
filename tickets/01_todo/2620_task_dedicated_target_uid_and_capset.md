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
- Grant `underwrap-ns` `cap_setuid` **only when at least one instance sets
  `isolateTargetUid = true`** (keep the cap surface minimal otherwise):
  `capabilities = "cap_sys_admin,cap_setpcap,cap_setuid+ep"`. `cap_setuid` is
  needed transiently in `underwrap-ns` and dropped before the target exec (2630).
  Document the addition in `docs/security.md` (why it's needed, that it's dropped
  pre-exec).

### Sidecar + protocol plumbing

The target uid/gid must reach `underwrap-ns`. The trust path is:
operator config → daemon → `ExecBundle` (over fd 3). Do **not** put it in env or
argv.

- `crates/underwrap-exec/src/main.rs` — extend the trusted instance sidecar
  (`/etc/underwrap/instances/<i>.yaml`, parsed by `parse_instance_config_text`,
  `InstanceConfig`) with optional `target_uid` / `target_gid`. The NixOS module
  writes these when `isolateTargetUid` is set.
- `crates/underwrap-protocol/src/exec.rs` — add `target_uid: Option<u32>` and
  `target_gid: Option<u32>` to `ExecBundle` (None ⇒ no isolation, current
  behavior). Populate it in `exec_via_ns` from the sidecar config.
- Validate in `underwrap-exec`: if present, the uid/gid must be > 0 and **not**
  equal to the caller's ruid/rgid (a target uid equal to the caller defeats the
  purpose) — `bail!` otherwise.

No behavior change yet in `underwrap-ns` beyond accepting/ignoring the new
fields (the switch is 2630). Keep the field optional so all current instances
and the round-trip tests are unaffected.

## Acceptance

- New per-instance options `isolateTargetUid`, `targetUid`, `targetGid` exist
  and are documented; with `isolateTargetUid = true` and defaults, the eval
  produces a system user `underwrap-target-<instance>` with a passwd entry.
- `getcap /run/wrappers/bin/underwrap-ns` includes `cap_setuid` **iff** some
  instance enables isolation; otherwise unchanged.
- `ExecBundle` carries `target_uid`/`target_gid`; `underwrap-exec` reads them
  from the sidecar and rejects a target uid equal to the caller's uid.
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
