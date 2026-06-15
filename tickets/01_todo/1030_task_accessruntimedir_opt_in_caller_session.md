# Task: Repurpose accessRuntimeDir as the default-off caller-session opt-in (non-recursive idmap)

Parent: `1000_story_runtime_dir_for_executing_uid`
Depends on: `1020_task_ns_xdg_runtime_dir_from_target_uid`

## Problem

`accessRuntimeDir` currently defaults **on** and idmaps the caller's
`/run/user/<caller-uid>` for every wrapper (commits `0a7c959` + `1a42767` on
`main`). With X getting its own runtime dir by default (1020), this flag's job
narrows to: *opt into the caller's real session* (wayland / D-Bus / ssh-agent),
for the few wrappers that need it (browser-auth: `aws-sso`, `acli`).

It also currently fails on a real desktop session: `idmap_dir` in
`crates/underwrap-ns/src/idmap.rs` uses `OPEN_TREE_CLONE | AT_RECURSIVE` +
`mount_setattr(..., AT_RECURSIVE)`, and `/run/user/<uid>` carries fuse submounts
(xdg-document-portal `/doc`, gvfsd-fuse `/gvfs`) whose filesystem lacks
`FS_ALLOW_IDMAP` → `mount_setattr` returns `EINVAL` for the whole tree.

## Design

- **Flip the default to `false`** (config serde default + NixOS option default).
  Remove the `default_true` helper added in `1a42767`.
- **Redefine the semantics:** when `true`, the target opts into the caller's
  session runtime dir. Concretely:
  - Point `XDG_RUNTIME_DIR` at the caller's `/run/user/<caller-uid>` (so tools
    find `$XDG_RUNTIME_DIR/bus`, `wayland-0`, etc. there), overriding the
    default X-own dir from 1020.
  - Idmap `/run/user/<caller-uid>` caller↔X into the namespace.
  - Skip silently if the caller's dir is absent (no logind session) — falls back
    to the default X-own dir rather than hard-failing.
- **Make the idmap non-recursive.** In `idmap_dir`, drop `AT_RECURSIVE` from both
  the `open_tree` flags and the `mount_setattr` flags, so only the named
  directory's own filesystem is cloned/idmapped and foreign (fuse) submounts are
  dropped instead of failing the operation. Document why (the fuse-portal case).
  Note this also makes the existing `accessPaths` idmap more robust (a fuse
  submount under `~` no longer breaks the whole home idmap) and slightly more
  conservative (never pulls a different filesystem into the target). Confirm the
  existing `accessPaths` VM cases (`test-iso-home`, `test-iso-dir`) are
  unaffected (single-fs dirs).
- Keep the libsecret-bus caveat (now scoped to this explicit opt-in): a target
  bypassing `DBUS_SESSION_BUS_ADDRESS` could reach the caller's real Secret
  Service — functional concern, not an exposure of the brokered secret to the
  caller. Do **not** block the combo (`aws-sso`/`acli` legitimately need it).

## Touch-points

- `crates/underwrap-protocol/src/config.rs` (`WrapperDef::access_runtime_dir`
  default + doc).
- `crates/underwrap-ns/src/main.rs` (`build_idmap_paths` / `caller_runtime_dir`
  and the `XDG_RUNTIME_DIR` branch) + `env.rs`.
- `crates/underwrap-ns/src/idmap.rs` (`idmap_dir`: non-recursive).
- `nix/modules/underwrap.nix` (`accessRuntimeDir` default + description).

## Acceptance

- Default wrappers do not idmap `/run/user`; only `accessRuntimeDir = true` ones
  do, and they get the caller's session dir as `XDG_RUNTIME_DIR`.
- The opt-in succeeds on a desktop session with fuse portals mounted under
  `/run/user/<uid>` (no `EINVAL`).
- Unit + VM coverage in 1040.
