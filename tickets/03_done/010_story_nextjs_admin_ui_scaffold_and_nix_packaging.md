# Story: Scaffold the Next.js admin UI and package it in Nix

Epic: `tickets/01_todo/epic_nextjs_admin_ui_replatform.md`

## Background

We want a real replacement, not a side experiment. The new UI needs a
first-class place in the repo, reproducible JS dependency management,
and a deployable Nix package/service shape.

## Scope

- Add a dedicated Next.js + TypeScript app to the repo.
- Set up the base app structure for server-side data fetching and route
  handling.
- Add the initial design system foundation (e.g. Tailwind and a small
  component layer) for a dark, high-contrast ops-console aesthetic.
- Choose the JS package manager/build strategy that fits Nix packaging
  best.
- Produce a Nix package that builds the app reproducibly and ships a
  deployable runtime artifact.
- Define the first-class NixOS integration path for the new app.
- Keep the public runtime/deployment name aligned with
  `underwrap-admin-ui` even if the source layout changes.

## Acceptance criteria

- [ ] A new Next.js app exists in the repo with TypeScript enabled.
- [ ] `flake.nix` builds the app reproducibly.
- [ ] The build output is suitable for running under systemd on NixOS.
- [ ] The visual/styling foundation is aligned with a dark, high-contrast
      ops console.
- [ ] The NixOS integration path is explicit enough to implement without
      deployment-model ambiguity.
- [ ] Static assets are included correctly in the package output.
- [ ] Local developer instructions exist for running the app in dev mode.
- [ ] The scaffold is ready for auth and page work without repo-layout
      churn.

## Child tasks

- `010_task_admin_ui_nextjs_app_scaffold_and_design_system.md`
- `020_task_admin_ui_nix_build_runtime_and_dev_shell.md`
