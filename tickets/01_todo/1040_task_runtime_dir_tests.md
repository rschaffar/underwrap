# Task: Tests for the runtime-dir redesign (unit + VM)

Parent: `1000_story_runtime_dir_for_executing_uid`
Depends on: `1010`, `1020`, `1030`

## Unit tests (`crates/underwrap-ns`, `crates/underwrap-protocol`)

- `build_target_env` / the runtime-dir derivation: default path yields
  `<base>/<X-component>` (the executing uid's dir), **not** the caller uid.
- The opt-in path yields the caller's `/run/user/<caller-uid>` and is gated on
  `access_runtime_dir`.
- `build_idmap_paths` (or its successor): opt-in adds the caller runtime dir;
  default does not; skip-if-absent preserved; uid resolved from the passed-in
  value, never the wire.
- Update the existing `main/tests.rs`, `env/tests.rs`, and protocol round-trip /
  config-default tests for the flipped `access_runtime_dir` default and any
  signature changes.

## VM test (`nix/tests/node.nix` + `nix/tests/test.py`)

Rework the existing `test-runtime-dir` / `test-runtime-dir-denied` fixtures:

- **Default path:** a wrapper with no `accessRuntimeDir` set writes under
  `XDG_RUNTIME_DIR` and the marker is owned by its **target uid** (not the
  caller), and the marker does **not** appear in the caller's `/run/user`-style
  dir. Confirm the dir is the provisioned per-target-uid one from 1010.
- **Session-less caller:** the default path still works when the caller has no
  `/run/user/<caller-uid>` (it must not depend on it).
- **Opt-in path (`accessRuntimeDir = true`):** the target reaches the caller's
  runtime dir. **Crucially**, add a regression guard for the bug that slipped
  through: mount a **non-idmappable submount** under the caller runtime dir
  before invoking (e.g. `mount -t proc proc <dir>/sub` — proc lacks
  `FS_ALLOW_IDMAP`, reproducing the fuse-portal case without needing a fuse
  daemon), and assert the wrapper still succeeds (non-recursive idmap drops the
  submount instead of `EINVAL`). Umount + clean up afterward.
- **Opt-out is genuine:** keep a denied/contained case if still meaningful under
  the new model.

Why this matters: the previous VM test only exercised a runtime dir with no
submounts, so it never caught the recursive-idmap `EINVAL` that broke a real
desktop session. The proc-submount guard closes that gap.

## Acceptance

- All unit tests + `nix build .#checks.x86_64-linux.vm-test` green.
- The proc-submount case fails without the non-recursive fix and passes with it.
