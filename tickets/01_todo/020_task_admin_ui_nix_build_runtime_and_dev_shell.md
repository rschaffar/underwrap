# Task: Package the Next.js admin UI with Nix and define its runtime shape

Parent: `010_story_nextjs_admin_ui_scaffold_and_nix_packaging.md`
Epic: `epic_nextjs_admin_ui_replatform.md`

## Scope

- Add reproducible Nix build support for the new app.
- Produce a deployable runtime artifact suitable for systemd.
- Add local developer workflow support for the JS app.
- Define the first-class NixOS service/module integration path for the new app.
- Keep the runtime/deployment identity aligned with `underwrap-admin-ui`.

## Acceptance criteria

- [ ] `flake.nix` builds the new app.
- [ ] Static assets are included correctly in the package output.
- [ ] The resulting artifact is suitable for NixOS service deployment.
- [ ] A first-class NixOS integration path is specified or implemented for deployment.
- [ ] Local dev instructions exist for the JS app.
