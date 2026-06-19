# Task: Per-(instance, wrapper) sandbox users + re-keyed authority table

Parent: `2720_story_per_wrapper_target_uid`
Blocks: `2740_task_runtime_isolation_key_instance_wrapper`

## Problem

`nix/modules/underwrap.nix` currently mints one sandbox user per **target
binary path** and writes a path-keyed `/etc/underwrap/target-uids`. Re-key both
to `(instance, wrapper)` so every wrapper gets its own uid and the debug tool
stops sharing a host-wide uid.

## Fix — all in `nix/modules/underwrap.nix`

### Enumerate pairs, not paths

- Replace `targetPaths` (`:460`) with a list of `(instance, wrapper, target)`
  records across all instances. Drop the special
  `"${cfg.package}/bin/underwrap-debug-tool"` entry from this list entirely —
  the debug variant of a wrapper now runs under **that wrapper's** uid (it
  brokers the same secret), so it needs no separate identity. The debug tool
  binary is still execable; only its *uid sourcing* changes (handled by the
  daemon emitting `read.target = debug_tool_target` while the
  `(instance, wrapper)` selector stays the wrapper's — see `2740`).

### Stable per-pair name

- `targetUserName` (`:472`) hashes `"${instance}/${wrapper}"` instead of the
  path:

  ```nix
  targetUserName = instance: wrapper:
    "underwrap-target-"
    + builtins.substring 0 12
        (builtins.hashString "sha256" "${instance}/${wrapper}");
  ```

  Still `underwrap-target-` (17) + 12 hex = 29 chars. Stable per pair; adding
  or removing other pairs never reshuffles an existing pair's uid.
- `targetUsers` / `targetGroups` (`:479`–`486`): one `isSystemUser` user +
  matching group per pair. Keep the existing hardening: member of **only** its
  own primary group (never an instance group), `/sbin/nologin`, no home.
  `description = "underwrap sandbox uid for <instance>/<wrapper> (<target>)"`.

### Re-key the authority table

- `targetUidTableText` (`:499`): emit four tab-separated columns per pair:

  ```
  <instance>\t<wrapper>\t<user>\t<group>
  ```

  Instance and wrapper names are NixOS attribute keys (no tabs), so the split
  stays unambiguous. Update the surrounding comment block (`:488`–`498`) and
  the `environment.etc` comment (`:720`) to describe `(instance, wrapper)`
  keying instead of "absolute target path." The table stays `root:root 0644`,
  the sole source of truth, never relayed.

### Runtime dirs (mechanism unchanged)

- `targetRuntimeTmpfiles` (`:512`) still provisions `<base>/<targetUserName>`
  per identity — now one per pair instead of per path. `underwrap-ns` derives
  the same dir from `getpwuid(X).pw_name`, so no runtime-side change. Just
  feed the new `targetUserName instance wrapper` into the tmpfiles map.

## Acceptance

- Eval with two wrappers pointing at the **same** target binary in one instance
  produces **two** distinct `underwrap-target-*` users; the same binary across
  two instances produces two more — four distinct uids total, none shared.
- `/etc/underwrap/target-uids` has one `<instance>\t<wrapper>\t<user>\t<group>`
  row per configured wrapper, `root:root 0644`, no row for the debug tool.
- Each sandbox user is a member of only its own primary group (assert in VM
  test — see `2750`).
- No `underwrap-target-<hash(path)>` user remains; `grep` for the old
  path-hash naming finds nothing.
- `nix flake check` / module eval green.

## Out of scope

- The `underwrap-ns` lookup and the exec-side selector plumbing (`2740`).
- VM-test assertions proving distinct uids (`2750`).

## Status: DONE (2720)

Module re-keyed to `(instance, wrapper)`: `targetPairs` enumerates pairs (debug-tool row dropped), `targetUserName instance wrapper` hashes `"<instance>/<wrapper>"`, the authority table emits 4-column `<instance>\t<wrapper>\t<user>\t<group>` rows, runtime-dir tmpfiles + the `environment.etc` gate moved to `targetPairs`. Eval-verified: three pairs sharing one binary (`local/gh`, `local/git`, `remote/gh`) resolve to three distinct uids. Full VM-test eval deferred to 2750 (the table reader in `underwrap-ns` lands in 2740 — one-shot cutover).
