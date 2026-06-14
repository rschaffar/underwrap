# Task 2710: update docs and tests for mandatory isolation

## Goal

Remove the optional-isolation security story and test matrix. Document and test the simpler first-release model: every target runs under a dedicated per-target UID.

## Scope

- Update `docs/security.md`:
  - remove `isolate = true` as a precondition.
  - state that target UID isolation is mandatory.
  - remove/replace the unisolated env-mode descendant caveat.
  - keep residual risks: root can inspect, target can exfiltrate its own secret, encrypted/absent swap still matters, narrow wrappers still matter.
- Update `docs/testing.md` accordingly.
- Update `examples/config.yaml` and any other user-facing examples.
- Update NixOS VM fixtures:
  - remove `isolate = true` settings because they no longer exist.
  - remove or rewrite tests that assert the old unisolated contrast.
  - keep strong assertions that target and descendants run as dedicated `underwrap-target-*` UIDs and deny same-caller `/proc`/ptrace/process memory access.
  - keep files/libsecret/idmapped access tests under the always-isolated model.
- Ensure logs/assertions no longer describe unisolated target behavior as a supported mode.

## Acceptance criteria

- Documentation has one clear model: wrapped targets are always dedicated-UID isolated.
- VM tests prove the mandatory boundary for env, files, and libsecret projections.
- No docs/examples/tests refer to configuring `isolate = true/false`.

## Suggested checks

- `cargo test --workspace`
- `nix build .#checks.x86_64-linux.vm-test`
