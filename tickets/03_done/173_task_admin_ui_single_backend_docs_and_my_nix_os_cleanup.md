# Task: Update docs and `~/nix/my_nix_os` for the single-backend admin UI model

Parent: `170_story_admin_ui_single_backend_simplification.md`

## Scope

- Update docs to describe the admin UI as one UI per backend.
- Remove stale language suggesting the operator UI is a multi-backend switchboard.
- Update `~/nix/my_nix_os` `shame01` config to the singular backend config shape.
- Rename confusing backend labels like `robert` to something backend/host oriented.
- Re-check the HTTP tailscale deployment path and bootstrap behavior after config cleanup.

## Key files likely involved

Repo docs:
- `docs/admin-ui.md`
- `apps/admin-ui/README.md`
- other docs mentioning target switching if still present

Local deployment repo:
- `~/nix/my_nix_os/hosts/shame01/configuration.nix`
- possibly `~/nix/my_nix_os/hosts/shame01/sops.nix`

## Handoff notes

Recent local `my_nix_os` work already moved `shame01` toward:
- local bootstrap state in `/var/lib/underwrap-admin-ui`
- no declarative operator password hash
- `UNDERWRAP_ADMIN_UI_SECURE_COOKIES=false` for HTTP tailscale use

This task should align the remaining naming/config structure with the final
single-backend model.

## Acceptance criteria

- [ ] Docs describe one admin UI per backend.
- [ ] `my_nix_os` uses the singular backend config shape.
- [ ] The backend label/name is host/environment oriented, not operator oriented.
- [ ] The deployment instructions match the actual supported shape.
