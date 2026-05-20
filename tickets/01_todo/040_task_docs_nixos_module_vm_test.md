# Task: NixOS module, docs, and VM test updates

Parent: [[000_story_one_resource_per_wrapper_frontend]]
Depends on: [[010_task_flatten_wrapper_config_schema]],
[[020_task_frontend_daemon_collapse]],
[[030_task_wrapper_flow_protocol_collapse]]

## Goal

Bring the NixOS module, example config, docs, and VM test in line with
the new "one resource per wrapper" shape.

## Changes

### `nix/modules/underwrap.nix`

- `services.underwrap.instances.<n>.wrappers.<w>` option set drops the
  `resources` submodule and gains:
  - `resource` (string, required)
  - `env` (bool, default false)
  - `files.paths` (list of strings, default [])
  - `libsecret` (bool, default false)
- Module-level assertion: `resource` is non-empty.
- The generated YAML matches the flat schema from
  [[010_task_flatten_wrapper_config_schema]].
- Any internal helpers in the module that map over a wrappers'
  resources attrset are collapsed to single-value access.

### `examples/config.yaml`

- Rewrite the annotated example wrappers to the flat shape.
- If the example previously demonstrated a multi-resource wrapper
  (e.g. `gh-authorized` with token + gws-state), split it into two
  wrappers — one per resource — to preserve the pedagogical coverage.

### `docs/security.md`

- Audit every section that mentions multiple resources per wrapper or
  the `ResourceUse` projection map. Update the slot table and any
  invariant text.
- The libsecret design block can stay; the constraint "one private bus
  per namespace" now matches the data model exactly.

### `docs/backend.md`

- If it describes the frontend wrapper config schema, update the
  example. The backend data model (`PairsDocument`, grant key) stays
  as-is and should be stated explicitly as the trust-boundary reason
  for not collapsing pairs/grants.

### `CLAUDE.md` (AGENTS.md)

- "Architecture notes → Grant model" / "Identity model" sections: add
  a line noting the frontend invariant (one resource per wrapper) and
  the explicit reason it does *not* extend to the backend grant key.

### `nix/tests/node.nix`

- Update wrapper definitions to the new option shape.
- Keep multi-slot coverage: at least one test wrapper exercises env +
  files + libsecret on its single resource (or split across wrappers
  if that's how the test was structured).

### `nix/tests/test.py`

- Adjust any spawn/assertion logic that depends on a wrapper having
  multiple resources. The 21 assertions should still pass; rebalance
  them if "multi-resource" was a covered case.

## Acceptance

- `nix build .#checks.x86_64-linux.vm-test` passes (full 21+ assertion
  run on a real NixOS VM).
- `examples/config.yaml` loads cleanly through `serde_yaml` (covered
  by an existing config-parse test or add one).
- All docs reflect the new shape; no stale references to
  `ResourceUse` or `wrapper.resources` remain (`rg ResourceUse docs/
  examples/ nix/`).
