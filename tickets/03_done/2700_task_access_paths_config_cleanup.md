# Task 2700: reframe idmapped paths as normal target access paths

## Goal

With mandatory target isolation, idmapped caller-directory access is no longer an optional feature that only matters under `isolate = true`; it is the normal way to grant an isolated target selected access to caller-owned config/state.

## Scope

Choose and apply the first-release config naming. Recommended direction:

- Rename Nix wrapper option `idmapPaths` to `accessPaths`.
- Consider whether the Rust/config field should remain `idmap_paths` internally or be renamed to match the product language.
- Update examples and option docs to explain that these paths are explicit caller-owned filesystem access grants for the dedicated target UID.
- Preserve the current semantics:
  - `~` and `~/rel` resolve against the calling user's home.
  - absolute paths are allowed.
  - `..` traversal and `~user` remain rejected.
  - only the caller uid/gid maps to the target uid/gid; other owners remain unmapped/overflow.

## Acceptance criteria

- User-facing Nix config no longer says the paths have "no effect unless isolate is set".
- The option name and docs make sense in an always-isolated model.
- Existing idmapped mount security properties remain unchanged.

## Suggested checks

- `cargo test -p underwrap-ns`
- `cargo test -p underwrap-protocol`
- update any examples under `examples/` and docs references
