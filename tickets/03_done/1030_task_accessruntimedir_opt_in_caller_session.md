# Task: Remove accessRuntimeDir entirely + best-effort per-submount idmap

Parent: `1000_story_runtime_dir_for_executing_uid`
Depends on: `1020_task_ns_xdg_runtime_dir_from_target_uid`

## Decision (2026-06-15, user)

The earlier plan repurposed `accessRuntimeDir` as a default-off opt-in for the
caller's session runtime dir. The user decided to **remove the feature
entirely** instead — too much surface/complexity for a need that hasn't
materialised. "Only if I run into too many inconvenience problems then I'll deal
with it later." The escape-hatch design (caller-session opt-in, with the
libsecret-bus conflict handled by forbidding the combo or shadowing the bus) is
recorded below for if/when it comes back.

## Part A — remove accessRuntimeDir (DONE)

`XDG_RUNTIME_DIR` is now always the target's own provisioned dir (1010/1020).
There is no caller-session opt-in and no idmap of `/run/user`.

Removed:

- `WrapperDef::access_runtime_dir` (config schema).
- `ExecBundle::access_runtime_dir` + `WrapperResponse::ReadOk::access_runtime_dir`
  (both wire structs + encode/decode).
- `underwrap-exec` `ReadResult::access_runtime_dir` + plumbing.
- daemon `wrapper_flow/read.rs` passthrough.
- ns `caller_runtime_dir` / `build_idmap_paths` / `CALLER_RUNTIME_DIR_BASE`;
  `idmap_paths` is now just `exec_bundle.access_paths.clone()`.
- NixOS `accessRuntimeDir` option + `access_runtime_dir` passthrough.
- All references in tests, docs, and `examples/config.yaml`.

## Part B — best-effort per-submount idmap (DONE)

`accessPaths` (`~`, `~/.config/gh`, …) must keep working when the directory
contains submounts. Two failure modes to avoid:

- A recursive `mount_setattr(AT_RECURSIVE)` is all-or-nothing: it `EINVAL`s the
  whole tree the moment it meets a submount whose filesystem lacks
  `FS_ALLOW_IDMAP` (fuse — portals, sshfs, AppImage).
- A non-recursive clone drops submounts entirely, so legitimate idmappable
  submounts (e.g. nested **ZFS** datasets under `~` on deepthought — OpenZFS
  ≥ 2.2 supports idmapped mounts) would vanish from the target's view.

`idmap_dir` therefore:

1. `open_tree(OPEN_TREE_CLONE | AT_RECURSIVE)` — recursive clone, so submounts
   come along.
2. idmaps the **named dir's own mount** with `mount_setattr` + `AT_EMPTY_PATH`
   (no `AT_RECURSIVE`). A non-idmappable root is a **hard error** (the access
   path is pointless otherwise).
3. enumerates submounts under the dir from `/proc/self/mountinfo` (parsed in the
   already-private mount ns) and idmaps each one individually. `EINVAL` (fs
   lacks `FS_ALLOW_IDMAP`) → **skip with a stderr warning**; any other error is
   fatal. A skipped submount stays cloned but un-idmapped, so its files present
   as their on-disk (caller) owner and are simply not caller-mapped for the
   target — never a failure, never an exposure.
4. `move_mount` attaches the idmapped tree over the dir.

New helpers (all in `crates/underwrap-ns/src/idmap.rs`):
`set_mount_idmap`, `submount_relpaths`, `submounts_under` (pure, unit-tested),
`unescape_mountinfo` (mountinfo octal escapes, unit-tested).

## Validation

- Unit tests: `submounts_under` (strict descendants, escaped mountpoints, no
  children) + `unescape_mountinfo` in `idmap/tests.rs`. Green.
- The per-mount idmap syscall behaviour (ZFS idmapped, fuse skipped) is an
  empirical/kernel concern — covered by the VM test in 1040 (proc submount as a
  portable stand-in for a non-idmappable fs) and validated for real on
  deepthought's ZFS home.

## If the caller-session opt-in comes back later

- Re-add a per-wrapper `accessRuntimeDir` (default off) that points
  `XDG_RUNTIME_DIR` at `/run/user/<caller-uid>` and idmaps it.
- libsecret conflict: a `libsecret` + caller-session combo makes the caller's
  real session bus visible at `$XDG_RUNTIME_DIR/bus`, so a target ignoring
  `DBUS_SESSION_BUS_ADDRESS` reaches the caller's real Secret Service. Resolve by
  **forbidding the combo** at config validation, or by bind-mounting the private
  bus over `$XDG_RUNTIME_DIR/bus`. (Recommended first-release stance: forbid.)
