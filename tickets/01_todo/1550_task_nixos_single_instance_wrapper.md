# Task: NixOS module — one wrapper per instance

Part of: `epic_single_instance_wrapper_redesign_2026-05`.
Depends on: `1500` (config shape).

## Problem

`nix/modules/underwrap.nix` currently generates, **per wrapper**: a
setgid binary (`<instance>-<name>`), a Unix group
(`underwrap-<instance>-<name>`), a daemon socket, a daemon
`WrapperDef` entry with `socket`+`group`, a per-wrapper sidecar YAML,
and (optionally) a whole second debug binary via `addUnsafeDebugTool`.
The redesign collapses all of that to **one per instance**.

## What to do

### 1. One setgid binary per instance

Generate a single `uw-<instance>` security wrapper, `setgid = true`,
`group = underwrap-<instance>`, `source = underwrap-exec`. For the
instance marked default (a new `services.underwrap.instances.<n>.default`
option, or the instance literally named `default` — pick one and
document it), also install a bare `uw` symlink/alias.

Drop the per-wrapper `cp` loop, `fullWrapperName`,
`fullDebugWrapperName`, `wrapperGroupName`, `debugToolTarget`, and
the `addUnsafeDebugTool` option entirely.

### 2. One group per instance

`users.groups` gets a single `underwrap-${instName}` entry (plus the
existing daemon group). The daemon user joins it so it can chown the
socket.

### 3. One socket + one daemon config

The generated `DaemonConfig` (the `runtimeConfig` let-binding) now
emits instance-level `socket`, `group`, `ns_binary`,
`debug_tool_target = "${cfg.package}/bin/underwrap-debug-tool"`,
`instance = instName`, `xdg_runtime_dir_base`, and the `wrappers`
map with the trimmed `WrapperDef` shape (no `socket`/`group`;
include `env_passthrough` / `env_set`). There is one config file per
instance — the per-wrapper sidecar YAMLs
(`etc/underwrap/wrappers/<name>.yaml`) are removed.

`frontend-exec` needs a tiny instance-level sidecar (socket +
ns_binary + xdg base) OR reads the same instance config — pick the
approach that matches how ticket 1520 implemented instance
detection, and keep them consistent.

### 4. PATH integration

`userBareEntries` / `mkUserBin`: each authorized user gets `uw`
(default instance) and/or `uw-<instance>` on PATH. Drop the
per-wrapper bare-name symlinks and the `debug-<wrapper>` symlinks —
wrappers are now invoked as `uw[-<instance>] <wrapper> ...` and
debug as `uw[-<instance>] debug <wrapper> ...`.

(Decision point: do we still want bare per-wrapper command names for
ergonomics, e.g. a `gh-authorized` shim that execs `uw <wrapper>`?
Default to **no** for the first cut — one `uw` entrypoint — and
revisit if operators want the shims. Note the decision in the
module comment.)

### 5. Reserved name

Add an assertion that no wrapper in any instance is named `debug`
(matches the daemon-side validation from ticket 1500), with a clear
eval-time error message.

### 6. `node.nix`

Update the test node config to the new option shape (no
`addUnsafeDebugTool`; wrappers no longer imply per-wrapper groups).
Confirm `nix flake check` evaluates.

## Out of scope

- The VM-test Python assertions (ticket 1570).
- Daemon / exec behavior (tickets 1520, 1530).

## Acceptance

- `nix flake check` passes.
- The generated daemon config for a sample instance has exactly one
  `socket`, one `group`, a `debug_tool_target`, and a `wrappers` map
  with no per-wrapper `socket`/`group`.
- `grep -RIn "addUnsafeDebugTool\|fullDebugWrapperName\|wrapperGroupName" nix/`
  returns nothing.
- `nix build .#checks.x86_64-linux.vm-test` builds (assertions
  reworked in ticket 1570).
