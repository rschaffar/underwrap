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
(default instance) and/or `uw-<instance>` on PATH as the real
entrypoint. The per-wrapper setgid binaries and the
`debug-<wrapper>` setgid binaries are gone.

**DECIDED (operator request): keep the per-tool command names via
plain convenience shims.** For every declared wrapper, generate two
unprivileged POSIX shell scripts on PATH that just forward to the
real binary, so the day-to-day UX matches the old per-wrapper
binaries:

- `<wrapper>`        → `exec uw-<instance> <wrapper> "$@"`
- `debug-<wrapper>`  → `exec uw-<instance> debug <wrapper> "$@"`

(Use `uw` instead of `uw-<instance>` inside the shims for the default
instance if that reads cleaner — but `uw-<instance>` is always
correct and unambiguous, so prefer it.)

Critical properties of these shims:

- They are **plain shell scripts, NOT security.wrappers** — no
  setgid, no file capabilities. All privilege and policy still live
  exclusively in `uw-<instance>` (setgid) and the daemon. The shim is
  pure ergonomics: it saves the user from typing the
  `uw-<instance> <wrapper>` form.
- `exec "$@"`-style forwarding so argv, exit status, and signals pass
  through unchanged.
- Generated per authorized user the same way the bare `uw` entry is
  (reuse `mkUserBin` / the same PATH dir), so they land next to `uw`.

**Naming collisions are the operator's responsibility, same as
before.** The old design already produced a binary literally named
`<wrapper>` per wrapper; two instances declaring the same wrapper
name collided on PATH then too. The module does **not** add
collision-avoidance logic for the shims — document this in a module
comment and move on. (The `debug-` prefix likewise mirrors the old
`debug-<wrapper>` binary names.)

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
- Each authorized user has, on PATH: `uw` / `uw-<instance>` (the real
  setgid entrypoint) plus, per wrapper, a plain `<wrapper>` shim and a
  `debug-<wrapper>` shim that `exec uw-<instance> [debug] <wrapper>
  "$@"`. The shims are unprivileged (no setgid / caps).
- `nix build .#checks.x86_64-linux.vm-test` builds (assertions
  reworked in ticket 1570).
