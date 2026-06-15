# Task: Tests for the runtime-dir redesign (unit + VM)

Parent: `1000_story_runtime_dir_for_executing_uid`
Depends on: `1010`, `1020`, `1030`

## Unit tests (`crates/underwrap-ns`, `crates/underwrap-protocol`) — DONE

- `target_runtime_dir`: `<base>/<target-user-name>` (uid 0 → `<base>/root` as a
  portable check); rejects a relative base.
- `build_target_env` exports the resolved `xdg_runtime_dir` verbatim and still
  ignores an env_set `XDG_RUNTIME_DIR` override.
- `submounts_under` (pure): strict descendants relative to base, escaped
  mountpoints, empty when no children; `unescape_mountinfo` octal decoding.
- `access_runtime_dir` removed from every protocol round-trip / struct literal;
  `examples/config.yaml` updated (else the shipped-config parse test fails).

## VM test (`nix/tests/node.nix` + `nix/tests/test.py`) — TODO

`accessRuntimeDir` is gone, so the `test-runtime-dir` / `test-runtime-dir-denied`
fixtures are replaced. New cases:

- **Default path:** an isolated wrapper writes under `XDG_RUNTIME_DIR`; the
  marker is owned by its **target uid** (not the caller), and `XDG_RUNTIME_DIR`
  is exactly the provisioned `<targetRuntimeDirBase>/<target-user-name>` from
  1010. The marker does **not** appear in the caller's `/run/user/<uid>`.
- **Session-less caller:** the default path still works when the caller has no
  `/run/user/<caller-uid>` (it must not depend on it).
- **Best-effort submount idmap (regression guard):** give a wrapper an
  `accessPaths` entry (a dir under the caller's home), mount a
  **non-idmappable submount** under it before invoking (`mount -t proc proc
  <dir>/sub` — proc lacks `FS_ALLOW_IDMAP`, a portable stand-in for the
  fuse-portal case), and assert the wrapper **still succeeds**: the parent dir is
  idmapped (target reads/writes a caller-owned file in it as the caller) and the
  proc submount is skipped (a warning, not `EINVAL`). Umount + clean up after.
  Optionally also assert an idmappable submount (e.g. a `tmpfs` or bind mount of
  an ext4 dir) under the access path IS visible/idmapped, mirroring the ZFS case.

Why this matters: the previous VM test only exercised a single-mount dir, so it
never caught the recursive-idmap `EINVAL` that broke a real desktop session. The
proc-submount guard closes that gap for `accessPaths`.

## Acceptance

- Unit tests green (DONE).
- VM test reworked; `nix build .#checks.x86_64-linux.vm-test` green.
- The proc-submount access-path case passes (submount skipped, not fatal).
