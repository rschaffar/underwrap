# Task: Provision a per-target-uid runtime directory in the NixOS module

Parent: `1000_story_runtime_dir_for_executing_uid`

## Problem

A dedicated `underwrap-target-*` uid X has no `/run/user/<uid>` (logind only
makes those for login sessions), and `underwrap-ns` cannot create one at runtime
(no `CAP_DAC_OVERRIDE`/`CAP_CHOWN`; `/run/user` is root-owned and logind's). So
the system must provision X a runtime dir. The module already owns the target
users (`targetUsers`, `targetUserName path`), so it is the right place.

## Design

- Pick a dedicated base outside logind's tree — proposed `/run/underwrap-runtime`
  (do **not** reuse `/run/user`). Make it the default for the runtime base knob
  (see 1020 for the protocol/config side); the module sets the daemon config's
  base to it.
- For **each** target user, emit a systemd-tmpfiles dir owned by that user,
  mode `0700`:
  `d /run/underwrap-runtime/<targetUserName> 0700 <targetUserName> <targetGroupName> -`
  Key the path on the **target user name** (known at module-eval time) rather
  than the auto-allocated numeric uid, so no uid pinning is needed.  `ns` will
  resolve the same path from X via `getpwuid(X).pw_name` (see 1020) — keep the
  two in sync (both use the target *user name* as the path component).
- Create the base dir too (`d /run/underwrap-runtime 0755 root root -`).
- Consider a tmpfiles age rule to bound growth of long-lived per-uid dirs
  (e.g. clean stale files), but do **not** auto-remove the dir itself.

## Notes / pitfalls

- The path component must match exactly what `ns` derives in 1020. Decide the
  scheme once (target user name vs. numeric uid) and record it in both tickets'
  implementations. User-name keying avoids needing the numeric uid at eval.
- `/run` is a tmpfs; tmpfiles recreates these on boot. Confirm ordering so the
  dirs exist before the first wrapper invocation (tmpfiles runs early; fine).
- Keep the existing `xdgRuntimeDirBase` assertions (absolute path) and update
  the default + description (1050 covers docs).

## Acceptance

- Each target user has a `0700`, self-owned runtime dir under the base after a
  rebuild.
- The daemon config carries the dedicated base.
- VM `node.nix` updated so the test target users get their dirs (1040 asserts
  behavior).
