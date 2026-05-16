# Task: NixOS VM end-to-end test for the new architecture

Parent: `260_story_cutover_and_documentation.md`
Epic: `epic_admin_ui_spa_rebuild_and_multiuser.md`

## Scope

Extend `nix/tests/node.nix` and `nix/tests/test.py` to cover:

- Boot a NixOS VM with backendd serving admin TLS on a self-signed cert.
- HTTPS GET `/` returns the SPA index page.
- Bootstrap an admin via the SPA API (curl from inside the VM against the local TLS endpoint).
- Create a dynamic store via the admin API.
- Create an operator with grant to the new store.
- Operator login, list stores, see only granted store, attempt to list ungranted store returns 404.
- Mutation write + reveal both audited with operator principal recorded.
- Declarative store from the NixOS option is visible and editing via the API is rejected with `declarative_immutable`.
- VM test still verifies existing wrapper-chain invariants (cap_sys_admin, PID namespace, no core dumps, spoofing resistance).

## Acceptance criteria

- [ ] All listed scenarios pass.
- [ ] `nix build .#checks.x86_64-linux.vm-test` is green.
- [ ] Test runtime budget: under 2× current runtime.
- [ ] The test uses a freshly-generated TLS cert per run (no committed cert).
