# Task: Add first-use admin bootstrap and support sane HTTP-tailscale deployment

Parent: `120_story_admin_ui_hardening_e2e_and_cutover.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Remove the requirement to declaratively provide operator password hashes.
- On first use, require bootstrap of the first admin username and password.
- Persist operator auth state locally in the admin UI service state, not in Nix-declared SOPS password-hash files.
- Keep backend bearer tokens server-side as before.
- Support a sane plain-HTTP tailscale-only deployment shape for hosts that intentionally do not terminate TLS in front of the UI.

## Acceptance criteria

- [ ] The runtime config no longer requires `operators.*.password_hash_file`.
- [ ] A fresh deployment with no operator state presents a first-use bootstrap flow.
- [ ] Bootstrapping creates the first admin account and enables normal login afterward.
- [ ] The supported NixOS deployment path provides a persistent local state directory for operator bootstrap state.
- [ ] `~/nix/my_nix_os` no longer wires a declarative password-hash secret for `shame01`.
- [ ] The `shame01` deployment shape is sane for plain HTTP over tailscale.
